use chrono::{DateTime, Utc};

use crate::domain::{TaskId, TaskStatus};
use crate::lifecycle::{AutomaticRetry, DownstreamFailure, JitterSample};

use super::publication_artifact::sync_directory;
use super::{PublicationOutcome, RetryResult, TransferError, TransferExecutor};

impl TransferExecutor {
    /// Removes Controller temporary artifacts before deleting the remote task workspace.
    ///
    /// # Errors
    /// Returns [`TransferError`] when durable snapshots, retry writes, or lifecycle writes fail.
    pub async fn cleanup(
        &self,
        task_id: TaskId,
        now: DateTime<Utc>,
        jitter: JitterSample,
    ) -> Result<PublicationOutcome, TransferError> {
        let (task, attempt) = self.snapshots(task_id).await?;
        if task.status == TaskStatus::Completed {
            return Ok(PublicationOutcome::Completed);
        }
        if task.status != TaskStatus::RemoteCleanup {
            return Err(TransferError::Conflict);
        }
        Self::require_retry_due(&task, &attempt, now)?;
        let temporary = self.config.temp_root.join(task.id.to_string());
        match tokio::fs::remove_dir_all(&temporary).await {
            Ok(()) => {
                if sync_directory(&self.config.temp_root).await.is_err() {
                    return self
                        .cleanup_retry(
                            &task,
                            &attempt,
                            DownstreamFailure::LocalCleanup,
                            now,
                            jitter,
                        )
                        .await;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                return self
                    .cleanup_retry(
                        &task,
                        &attempt,
                        DownstreamFailure::LocalCleanup,
                        now,
                        jitter,
                    )
                    .await;
            }
        }
        self.delete_remote_workspace(&task, &attempt, now, jitter)
            .await
    }

    pub(super) async fn cleanup_retry(
        &self,
        task: &crate::persistence::TaskRecord,
        attempt: &crate::persistence::AttemptRecord,
        failure: DownstreamFailure,
        now: DateTime<Utc>,
        jitter: JitterSample,
    ) -> Result<PublicationOutcome, TransferError> {
        Ok(
            match self
                .retry(task, attempt, AutomaticRetry::Cleanup, failure, now, jitter)
                .await?
            {
                RetryResult::Scheduled {
                    retry_count,
                    next_retry_at,
                } => PublicationOutcome::RetryScheduled {
                    retry_count,
                    next_retry_at,
                },
                RetryResult::Failed => PublicationOutcome::Failed,
            },
        )
    }
}
