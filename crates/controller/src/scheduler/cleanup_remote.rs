use chrono::{DateTime, Utc};

use crate::domain::{FailureCode, FailureStage, TaskStatus};
use crate::lifecycle::{
    AdvanceCommand, DownstreamFailure, JitterSample, LifecycleFailure, LifecycleService,
};
use crate::persistence::{AttemptRecord, TaskRecord};
use crate::remote::{FileApiPath, VidenoaClient, VidenoaClientError};

use super::{PublicationOutcome, TransferError, TransferExecutor};

impl TransferExecutor {
    pub(super) async fn delete_remote_workspace(
        &self,
        task: &TaskRecord,
        attempt: &AttemptRecord,
        now: DateTime<Utc>,
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
        let client = VidenoaClient::new(
            worker.api_url,
            self.config.remote_timeouts,
            self.config.payload_limits,
        )?;
        let workspace = FileApiPath::parse(&task.id.to_string())?;
        match client.delete_file(&workspace).await {
            Ok(()) | Err(VidenoaClientError::NotFound) => {
                LifecycleService::new(self.resources.store.clone())
                    .advance(task, attempt, AdvanceCommand::FinishCleanup, now)
                    .await?;
                Ok(PublicationOutcome::Completed)
            }
            Err(
                VidenoaClientError::ServerStatus { .. }
                | VidenoaClientError::Network
                | VidenoaClientError::Timeout
                | VidenoaClientError::Stall,
            ) => {
                self.cleanup_retry(task, attempt, DownstreamFailure::RemoteCleanup, now, jitter)
                    .await
            }
            Err(
                VidenoaClientError::Conflict
                | VidenoaClientError::RateLimited
                | VidenoaClientError::ClientStatus { .. }
                | VidenoaClientError::UnexpectedStatus { .. }
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
                        now,
                    )
                    .await?;
                Ok(PublicationOutcome::Failed)
            }
        }
    }
}
