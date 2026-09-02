use chrono::{DateTime, Utc};

use crate::domain::{TaskId, TaskStatus};
use crate::lifecycle::{
    AdvanceCommand, AutomaticRetry, DownstreamFailure, JitterSample, LifecycleService,
    UploadEvidence,
};
use crate::persistence::InputIdentity;
use crate::remote::{sibling_output_path, FileApiPath, VidenoaClient, VidenoaClientError};

use super::{RetryResult, TransferError, TransferExecutor, UploadOutcome};

impl TransferExecutor {
    /// Streams and reconciles one durable upload stage.
    ///
    /// # Errors
    /// Returns [`TransferError`] when state, local input, persistence, or remote I/O fails.
    pub async fn upload(
        &self,
        task_id: TaskId,
        now: DateTime<Utc>,
        jitter: JitterSample,
    ) -> Result<UploadOutcome, TransferError> {
        let (mut task, mut attempt) = self.snapshots(task_id).await?;
        let worker_id = attempt
            .attempt
            .worker_id
            .ok_or(TransferError::MissingEvidence)?;
        let _permit = self
            .resources
            .coordinator
            .try_upload(worker_id)
            .ok_or(TransferError::Busy)?;
        match task.status {
            TaskStatus::Reserved => {
                LifecycleService::new(self.resources.store.clone())
                    .advance(&task, &attempt, AdvanceCommand::StartUpload, now)
                    .await?;
                (task, attempt) = self.snapshots(task_id).await?;
            }
            TaskStatus::Uploading => {}
            TaskStatus::Queued
            | TaskStatus::Staged
            | TaskStatus::Submitting
            | TaskStatus::Processing
            | TaskStatus::RemoteCompleted
            | TaskStatus::Downloading
            | TaskStatus::Verifying
            | TaskStatus::Publishing
            | TaskStatus::RemoteCleanup
            | TaskStatus::Completed
            | TaskStatus::Failed
            | TaskStatus::Cancelled => return Err(TransferError::Conflict),
        }
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
        let api_path = FileApiPath::parse(&format!(
            "{}/input.{}",
            task.id,
            task.input_extension.as_str()
        ))?;
        let rooted = self
            .resources
            .paths
            .open_input(task.request.input_path.as_str())?;
        let identity = InputIdentity::new(rooted.snapshot().platform_identity());
        if rooted.snapshot().length != task.input_size
            || task.input_identity != Some(identity)
            || DateTime::<Utc>::from(rooted.snapshot().modified).timestamp_millis()
                != task.input_mtime.timestamp_millis()
        {
            return Err(TransferError::Conflict);
        }
        let file = tokio::fs::File::from_std(rooted.reopen_checked()?.into_std());
        let uploaded = client.upload(&api_path, task.input_size, file).await;
        let stat = client.stat(&api_path).await;
        match stat {
            Ok(stat) if stat.is_file && stat.size == task.input_size => {
                let remote_input_path = match uploaded {
                    Ok(receipt) if receipt.size == task.input_size => receipt.path,
                    Ok(_) | Err(_) => stat.path,
                };
                let evidence = UploadEvidence {
                    remote_output_path: sibling_output_path(
                        &remote_input_path,
                        &format!("output.{}", task.output_extension.as_str()),
                    )?,
                    remote_input_path,
                };
                LifecycleService::new(self.resources.store.clone())
                    .advance(
                        &task,
                        &attempt,
                        AdvanceCommand::FinishUpload(evidence.clone()),
                        now,
                    )
                    .await?;
                Ok(UploadOutcome::Staged(evidence))
            }
            Ok(_) => {
                self.delete_owned_partial(&client, &api_path).await?;
                self.upload_retry(&task, &attempt, now, jitter).await
            }
            Err(VidenoaClientError::NotFound) => {
                self.upload_retry(&task, &attempt, now, jitter).await
            }
            Err(_) => self.upload_retry(&task, &attempt, now, jitter).await,
        }
    }

    async fn delete_owned_partial(
        &self,
        client: &VidenoaClient,
        path: &FileApiPath,
    ) -> Result<(), TransferError> {
        match client.delete_file(path).await {
            Ok(()) | Err(VidenoaClientError::NotFound) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    async fn upload_retry(
        &self,
        task: &crate::persistence::TaskRecord,
        attempt: &crate::persistence::AttemptRecord,
        now: DateTime<Utc>,
        jitter: JitterSample,
    ) -> Result<UploadOutcome, TransferError> {
        Ok(
            match self
                .retry(
                    task,
                    attempt,
                    AutomaticRetry::Upload,
                    DownstreamFailure::Upload,
                    now,
                    jitter,
                )
                .await?
            {
                RetryResult::Scheduled {
                    retry_count,
                    next_retry_at,
                } => UploadOutcome::RetryScheduled {
                    retry_count,
                    next_retry_at,
                },
                RetryResult::Failed => UploadOutcome::Failed,
            },
        )
    }
}
