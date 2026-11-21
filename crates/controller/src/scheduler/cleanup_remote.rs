use chrono::{DateTime, Utc};

use crate::domain::{FailureCode, FailureStage, TaskStatus};
use crate::lifecycle::{
    AdvanceCommand, DownstreamFailure, JitterSample, LifecycleFailure, LifecycleService,
};
use crate::persistence::{AttemptRecord, TaskRecord};
use crate::remote::{FileApiPath, VidenoaClient, VidenoaClientError};

use super::{PublicationOutcome, TransferCheckpointPoint, TransferError, TransferExecutor};

impl TransferExecutor {
    pub(super) async fn delete_remote_workspace(
        &self,
        task: &TaskRecord,
        attempt: &AttemptRecord,
        now: DateTime<Utc>,
        started: tokio::time::Instant,
        jitter: JitterSample,
    ) -> Result<PublicationOutcome, TransferError> {
        let worker_id = attempt
            .attempt
            .worker_id
            .ok_or(TransferError::MissingEvidence)?;
        let worker = self
            .resources
            .store
            .worker(worker_id)
            .await?
            .ok_or(TransferError::MissingEvidence)?;
        let client = VidenoaClient::new_with_password(
            worker.api_url,
            self.config.runtime_settings.remote_timeouts(),
            self.config.payload_limits,
            worker.password.as_ref(),
        )?;
        let workspace = FileApiPath::parse(&task.id.to_string())?;
        let deleted = client.delete_file(&workspace).await;
        if let Err(error) = &deleted {
            if *error != VidenoaClientError::NotFound {
                tracing::warn!(task_id = %task.id, attempt_id = %attempt.attempt.id,
                    %worker_id, operation = "cleanup.delete_remote_workspace", reason = %error,
                    "Task stage operation failed");
            }
        }
        let observed = self.observed_now(now, started);
        match deleted {
            Ok(()) | Err(VidenoaClientError::NotFound) => {
                self.checkpoint(TransferCheckpointPoint::RemoteDeleteSucceeded)
                    .await;
                LifecycleService::new(self.resources.store.clone())
                    .advance(task, attempt, AdvanceCommand::FinishCleanup, observed)
                    .await?;
                Ok(PublicationOutcome::Completed)
            }
            // The worker, not the cleanup, failed: retry later.
            Err(
                VidenoaClientError::ServerStatus { .. }
                | VidenoaClientError::Network
                | VidenoaClientError::Timeout
                | VidenoaClientError::Stall
                | VidenoaClientError::RateLimited
                | VidenoaClientError::ClientStatus {
                    status: 401 | 403, ..
                }
                | VidenoaClientError::UnexpectedStatus { .. }
                | VidenoaClientError::Incompatible,
            ) => {
                self.cleanup_retry(
                    task,
                    attempt,
                    DownstreamFailure::RemoteCleanup,
                    observed,
                    jitter,
                )
                .await
            }
            Err(
                VidenoaClientError::Conflict
                | VidenoaClientError::ClientStatus { .. }
                | VidenoaClientError::MalformedPayload
                | VidenoaClientError::OversizedPayload { .. }
                | VidenoaClientError::LocalIo
                | VidenoaClientError::InvalidFilePath
                | VidenoaClientError::EndpointUrl,
            ) => {
                LifecycleService::new(self.resources.store.clone())
                    .fail(
                        task,
                        Some(attempt),
                        LifecycleFailure::terminal(
                            TaskStatus::RemoteCleanup,
                            FailureStage::RemoteCleanup,
                            FailureCode::CleanupFailed,
                            "remote workspace cleanup was rejected",
                        ),
                        observed,
                    )
                    .await?;
                Ok(PublicationOutcome::Failed)
            }
        }
    }
}
