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
        let started = tokio::time::Instant::now();
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
                    .advance(
                        &task,
                        &attempt,
                        AdvanceCommand::StartUpload,
                        self.observed_now(now, started),
                    )
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
        let client = VidenoaClient::new_with_password(
            worker.api_url,
            self.config.runtime_settings.remote_timeouts(),
            self.config.payload_limits,
            worker.password.as_ref(),
        )?;
        let api_path = FileApiPath::parse(&format!(
            "{}/input.{}",
            task.id,
            task.input_extension.as_str()
        ))?;
        let context = UploadContext {
            task: &task,
            attempt: &attempt,
            client: &client,
            api_path: &api_path,
            now,
            started,
            jitter,
        };
        if restarting {
            return self.reconcile_restarting_upload(worker_id, context).await;
        }
        self.upload_fresh(context).await
    }

    async fn reconcile_restarting_upload(
        &self,
        worker_id: crate::domain::WorkerId,
        context: UploadContext<'_>,
    ) -> Result<UploadOutcome, TransferError> {
        match context.client.stat(context.api_path).await {
            Ok(stat) if stat.is_file && stat.size == context.task.input_size => {
                self.finish_upload(
                    context.task,
                    context.attempt,
                    stat.path,
                    self.observed_now(context.now, context.started),
                )
                .await
            }
            Ok(_) => self.cleanup_and_retry(&context).await,
            Err(VidenoaClientError::NotFound) => self.upload_fresh(context).await,
            Err(error) => {
                tracing::warn!(
                    task_id = %context.task.id,
                    %worker_id,
                    error = %error,
                    "Upload recovery request failed"
                );
                self.upload_retry(
                    context.task,
                    context.attempt,
                    self.observed_now(context.now, context.started),
                    context.jitter,
                )
                .await
            }
        }
    }

    pub(super) async fn cleanup_and_retry(
        &self,
        context: &UploadContext<'_>,
    ) -> Result<UploadOutcome, TransferError> {
        let cleanup = self
            .delete_owned_partial(context.client, context.api_path)
            .await;
        let outcome = self
            .upload_retry(
                context.task,
                context.attempt,
                self.observed_now(context.now, context.started),
                context.jitter,
            )
            .await?;
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
        self.checkpoint(super::TransferCheckpointPoint::UploadCompleted)
            .await;
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
                    "local input is unavailable or is not a permitted regular file",
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
