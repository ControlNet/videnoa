use chrono::{DateTime, Utc};

use crate::domain::{FailureCode, FailureStage, TaskId, TaskStatus};
use crate::lifecycle::{
    AdvanceCommand, AutomaticRetry, DownstreamFailure, JitterSample, LifecycleFailure,
    LifecycleService, UploadEvidence,
};
use crate::remote::{sibling_output_path, FileApiPath, VidenoaClient, VidenoaClientError};

use super::upload_fresh::UploadContext;
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
        Self::require_retry_due(&task, &attempt, now)?;
        let restarting = task.status == TaskStatus::Uploading;
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
        if restarting {
            return match client.stat(&api_path).await {
                Ok(stat) if stat.is_file && stat.size == task.input_size => {
                    self.finish_upload(&task, &attempt, stat.path, now).await
                }
                Ok(_) => {
                    self.cleanup_and_retry(&client, &api_path, &task, &attempt, now, jitter)
                        .await
                }
                Err(VidenoaClientError::NotFound) => {
                    self.upload_retry(&task, &attempt, now, jitter).await
                }
                Err(_) => self.upload_retry(&task, &attempt, now, jitter).await,
            };
        }
        self.upload_fresh(UploadContext {
            task: &task,
            attempt: &attempt,
            client: &client,
            api_path: &api_path,
            now,
            jitter,
        })
        .await
    }

    pub(super) async fn cleanup_and_retry(
        &self,
        client: &VidenoaClient,
        path: &FileApiPath,
        task: &crate::persistence::TaskRecord,
        attempt: &crate::persistence::AttemptRecord,
        now: DateTime<Utc>,
        jitter: JitterSample,
    ) -> Result<UploadOutcome, TransferError> {
        let cleanup = self.delete_owned_partial(client, path).await;
        let outcome = self.upload_retry(task, attempt, now, jitter).await?;
        cleanup?;
        Ok(outcome)
    }

    pub(super) async fn finish_upload(
        &self,
        task: &crate::persistence::TaskRecord,
        attempt: &crate::persistence::AttemptRecord,
        remote_input_path: crate::domain::RemotePath,
        now: DateTime<Utc>,
    ) -> Result<UploadOutcome, TransferError> {
        let evidence = UploadEvidence {
            remote_output_path: sibling_output_path(
                &remote_input_path,
                &format!("output.{}", task.output_extension.as_str()),
            )?,
            remote_input_path,
        };
        LifecycleService::new(self.resources.store.clone())
            .advance(
                task,
                attempt,
                AdvanceCommand::FinishUpload(evidence.clone()),
                now,
            )
            .await?;
        Ok(UploadOutcome::Staged(evidence))
    }

    pub(super) async fn upload_input_failure(
        &self,
        task: &crate::persistence::TaskRecord,
        attempt: &crate::persistence::AttemptRecord,
        code: FailureCode,
        now: DateTime<Utc>,
    ) -> Result<UploadOutcome, TransferError> {
        LifecycleService::new(self.resources.store.clone())
            .fail(
                task,
                Some(attempt),
                LifecycleFailure::terminal(
                    TaskStatus::Uploading,
                    FailureStage::Upload,
                    code,
                    "local input changed after task admission",
                ),
                now,
            )
            .await?;
        Ok(UploadOutcome::Failed)
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

    pub(super) async fn upload_retry(
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
