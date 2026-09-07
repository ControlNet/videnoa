use chrono::{DateTime, Utc};

use crate::domain::{FailureCode, FailureStage, TaskId, TaskStatus};
use crate::lifecycle::{JitterSample, LifecycleErrorCode, LifecycleFailure, LifecycleService};
use crate::recovery::{RecoveryCommandKind, RecoveryReport};

use super::{DownloadOutcome, PublicationOutcome, TransferError, TransferExecutor, UploadOutcome};

impl TransferExecutor {
    /// Executes transfer commands emitted from durable startup reconciliation.
    ///
    /// # Errors
    /// Returns an error when admitted transfer work cannot reach a durable outcome.
    pub async fn dispatch_recovery(
        &self,
        report: &RecoveryReport,
        now: DateTime<Utc>,
        jitter: JitterSample,
    ) -> Result<Vec<crate::domain::TaskId>, TransferError> {
        let mut advanced = Vec::new();
        for trace in report.traces() {
            match trace.command {
                RecoveryCommandKind::Upload => {
                    if self.upload_deferred_by_pause(trace.task_id).await? {
                        continue;
                    }
                    match self.upload(trace.task_id, now, jitter).await {
                        Ok(UploadOutcome::Staged(_)) => advanced.push(trace.task_id),
                        Ok(UploadOutcome::RetryScheduled { .. } | UploadOutcome::Failed)
                        | Err(TransferError::Busy | TransferError::RetryNotDue) => {}
                        Err(TransferError::Lifecycle(error))
                            if error.code() == LifecycleErrorCode::Conflict =>
                        {
                            if !self.upload_deferred_by_pause(trace.task_id).await? {
                                return Err(TransferError::Lifecycle(error));
                            }
                        }
                        Err(TransferError::MissingEvidence) => {
                            self.fail_malformed_recovery(trace.task_id, now).await?;
                        }
                        Err(error) => return Err(error),
                    }
                }
                RecoveryCommandKind::Download => {
                    match Box::pin(self.download(trace.task_id, now, jitter)).await {
                        Ok(DownloadOutcome::Verified(_)) => advanced.push(trace.task_id),
                        Ok(DownloadOutcome::RetryScheduled { .. } | DownloadOutcome::Failed)
                        | Err(TransferError::Busy | TransferError::RetryNotDue) => {}
                        Err(TransferError::MissingEvidence) => {
                            self.fail_malformed_recovery(trace.task_id, now).await?;
                        }
                        Err(error) => return Err(error),
                    }
                }
                RecoveryCommandKind::Verify | RecoveryCommandKind::Publish => {
                    match Box::pin(self.publish(trace.task_id, now, jitter)).await {
                        Ok(PublicationOutcome::Completed) => advanced.push(trace.task_id),
                        Ok(
                            PublicationOutcome::RetryScheduled { .. } | PublicationOutcome::Failed,
                        )
                        | Err(TransferError::RetryNotDue) => {}
                        Err(TransferError::MissingEvidence) => {
                            self.fail_malformed_recovery(trace.task_id, now).await?;
                        }
                        Err(error) => return Err(error),
                    }
                }
                RecoveryCommandKind::Cleanup => {
                    match self.cleanup(trace.task_id, now, jitter).await {
                        Ok(PublicationOutcome::Completed) => advanced.push(trace.task_id),
                        Ok(
                            PublicationOutcome::RetryScheduled { .. } | PublicationOutcome::Failed,
                        )
                        | Err(TransferError::RetryNotDue) => {}
                        Err(TransferError::MissingEvidence) => {
                            self.fail_malformed_recovery(trace.task_id, now).await?;
                        }
                        Err(error) => return Err(error),
                    }
                }
                RecoveryCommandKind::AwaitReservation
                | RecoveryCommandKind::Submit
                | RecoveryCommandKind::Poll
                | RecoveryCommandKind::Terminal => {}
            }
        }
        Ok(advanced)
    }

    async fn upload_deferred_by_pause(&self, task_id: TaskId) -> Result<bool, TransferError> {
        let paused = self.resources.store.config_manager().scheduler().paused;
        let task = self
            .resources
            .store
            .task(task_id)
            .await?
            .ok_or(TransferError::MissingEvidence)?;
        Ok(paused && task.status == TaskStatus::Reserved)
    }

    async fn fail_malformed_recovery(
        &self,
        task_id: TaskId,
        now: DateTime<Utc>,
    ) -> Result<(), TransferError> {
        let task = self
            .resources
            .store
            .task(task_id)
            .await?
            .ok_or(TransferError::MissingEvidence)?;
        let attempt = self.resources.store.current_attempt(task_id).await?;
        let stage = match task.status {
            TaskStatus::Verifying => FailureStage::Verification,
            TaskStatus::Publishing => FailureStage::Publication,
            TaskStatus::RemoteCleanup => FailureStage::RemoteCleanup,
            TaskStatus::Queued
            | TaskStatus::Reserved
            | TaskStatus::Uploading
            | TaskStatus::Staged
            | TaskStatus::Submitting
            | TaskStatus::Processing
            | TaskStatus::RemoteCompleted
            | TaskStatus::Downloading
            | TaskStatus::Completed
            | TaskStatus::Failed
            | TaskStatus::Cancelled => return Err(TransferError::Conflict),
        };
        LifecycleService::new(self.resources.store.clone())
            .fail_recovery(
                &task,
                attempt.as_ref(),
                LifecycleFailure::terminal(
                    task.status,
                    stage,
                    FailureCode::RemoteStateAmbiguous,
                    "durable recovery evidence is incomplete",
                ),
                now,
            )
            .await?;
        Ok(())
    }
}
