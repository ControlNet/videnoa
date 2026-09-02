use chrono::{DateTime, Utc};

use crate::domain::{TaskId, TaskStatus};
use crate::lifecycle::{JitterSample, LifecycleErrorCode};
use crate::recovery::{RecoveryCommandKind, RecoveryReport};

use super::{DownloadOutcome, TransferError, TransferExecutor, UploadOutcome};

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
                        Err(error) => return Err(error),
                    }
                }
                RecoveryCommandKind::Download => {
                    match Box::pin(self.download(trace.task_id, now, jitter)).await {
                        Ok(DownloadOutcome::Verified(_)) => advanced.push(trace.task_id),
                        Ok(DownloadOutcome::RetryScheduled { .. } | DownloadOutcome::Failed)
                        | Err(TransferError::Busy | TransferError::RetryNotDue) => {}
                        Err(error) => return Err(error),
                    }
                }
                RecoveryCommandKind::AwaitReservation
                | RecoveryCommandKind::Submit
                | RecoveryCommandKind::Poll
                | RecoveryCommandKind::Verify
                | RecoveryCommandKind::Publish
                | RecoveryCommandKind::Cleanup
                | RecoveryCommandKind::Terminal => {}
            }
        }
        Ok(advanced)
    }

    async fn upload_deferred_by_pause(&self, task_id: TaskId) -> Result<bool, TransferError> {
        let paused = self.resources.store.settings().await?.scheduler.paused;
        let task = self
            .resources
            .store
            .task(task_id)
            .await?
            .ok_or(TransferError::MissingEvidence)?;
        Ok(paused && task.status == TaskStatus::Reserved)
    }
}
