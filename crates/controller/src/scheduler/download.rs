use crate::domain::{FailureCode, FailureStage, TaskId, TaskStatus};
use crate::lifecycle::{
    AdvanceCommand, AutomaticRetry, DownloadEvidence, DownstreamFailure, JitterSample,
    LifecycleFailure, LifecycleService,
};
use crate::remote::{sibling_output_path, FileApiPath, VidenoaClient};
use chrono::{DateTime, Utc};

use super::{
    download_artifact::{download_artifact, recover_verified, DownloadArtifact},
    DownloadOutcome, RetryResult, TransferError, TransferExecutor,
};

impl TransferExecutor {
    /// Streams and verifies one durable download stage.
    ///
    /// # Errors
    /// Returns [`TransferError`] when state, persistence, local artifact I/O, or remote I/O fails.
    pub async fn download(
        &self,
        task_id: TaskId,
        now: DateTime<Utc>,
        jitter: JitterSample,
    ) -> Result<DownloadOutcome, TransferError> {
        let (mut task, mut attempt) = self.snapshots(task_id).await?;
        Self::require_retry_due(&task, &attempt, now)?;
        let Some(remote_output_path) = attempt.attempt.remote_output_path.clone() else {
            return self.download_ambiguity(&task, &attempt, now).await;
        };
        let Some(remote_input_path) = attempt.attempt.remote_input_path.as_ref() else {
            return self.download_ambiguity(&task, &attempt, now).await;
        };
        let expected_output_path = sibling_output_path(
            remote_input_path,
            &format!("output.{}", task.output_extension.as_str()),
        )?;
        if attempt.attempt.remote_job_id.is_none() || remote_output_path != expected_output_path {
            return self.download_ambiguity(&task, &attempt, now).await;
        }
        let _permit = self
            .resources
            .coordinator
            .try_download()
            .ok_or(TransferError::Busy)?;
        match task.status {
            TaskStatus::RemoteCompleted => {
                LifecycleService::new(self.resources.store.clone())
                    .advance(&task, &attempt, AdvanceCommand::StartDownload, now)
                    .await?;
                (task, attempt) = self.snapshots(task_id).await?;
            }
            TaskStatus::Downloading => {}
            TaskStatus::Queued
            | TaskStatus::Reserved
            | TaskStatus::Uploading
            | TaskStatus::Staged
            | TaskStatus::Submitting
            | TaskStatus::Processing
            | TaskStatus::Verifying
            | TaskStatus::Publishing
            | TaskStatus::RemoteCleanup
            | TaskStatus::Completed
            | TaskStatus::Failed
            | TaskStatus::Cancelled => return Err(TransferError::Conflict),
        }
        let directory = self.config.temp_root.join(task.id.to_string());
        let artifact = if let Some(artifact) =
            recover_verified(&directory, task.output_extension.as_str()).await?
        {
            artifact
        } else {
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
            let remote_path = FileApiPath::parse(&format!(
                "{}/output.{}",
                task.id,
                task.output_extension.as_str()
            ))?;
            let stat = match client.stat(&remote_path).await {
                Ok(stat) if stat.is_file && stat.size > 0 => stat,
                Ok(_) | Err(_) => {
                    return self.download_retry(&task, &attempt, now, jitter).await;
                }
            };
            let Ok(artifact) = Box::pin(download_artifact(DownloadArtifact {
                client: &client,
                remote_path: &remote_path,
                directory,
                extension: task.output_extension.as_str(),
                expected_size: stat.size,
            }))
            .await
            else {
                return self.download_retry(&task, &attempt, now, jitter).await;
            };
            artifact
        };
        LifecycleService::new(self.resources.store.clone())
            .advance(
                &task,
                &attempt,
                AdvanceCommand::FinishDownload(DownloadEvidence {
                    size: artifact.size,
                    sha256: artifact.sha256,
                }),
                now,
            )
            .await?;
        Ok(DownloadOutcome::Verified(artifact))
    }

    async fn download_ambiguity(
        &self,
        task: &crate::persistence::TaskRecord,
        attempt: &crate::persistence::AttemptRecord,
        now: DateTime<Utc>,
    ) -> Result<DownloadOutcome, TransferError> {
        LifecycleService::new(self.resources.store.clone())
            .fail(
                task,
                Some(attempt),
                LifecycleFailure::terminal(
                    task.status,
                    FailureStage::Download,
                    FailureCode::RemoteStateAmbiguous,
                    "durable download evidence does not identify the remote output",
                ),
                now,
            )
            .await?;
        Ok(DownloadOutcome::Failed)
    }

    async fn download_retry(
        &self,
        task: &crate::persistence::TaskRecord,
        attempt: &crate::persistence::AttemptRecord,
        now: DateTime<Utc>,
        jitter: JitterSample,
    ) -> Result<DownloadOutcome, TransferError> {
        Ok(
            match self
                .retry(
                    task,
                    attempt,
                    AutomaticRetry::Download,
                    DownstreamFailure::Download,
                    now,
                    jitter,
                )
                .await?
            {
                RetryResult::Scheduled {
                    retry_count,
                    next_retry_at,
                } => DownloadOutcome::RetryScheduled {
                    retry_count,
                    next_retry_at,
                },
                RetryResult::Failed => DownloadOutcome::Failed,
            },
        )
    }
}
