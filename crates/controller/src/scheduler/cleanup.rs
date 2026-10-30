use chrono::{DateTime, Utc};

use crate::domain::{TaskId, TaskStatus};
use crate::lifecycle::{AutomaticRetry, DownstreamFailure, JitterSample, Lifecycle, RetryMode};

use super::diagnostics::OperationError;
use super::{
    PublicationOutcome, RetryResult, TransferCheckpointPoint, TransferError, TransferExecutor,
};

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
        self.checkpoint(TransferCheckpointPoint::BeforeLocalCleanup)
            .await;
        if let Err(error) = remove_task_workspace(&self.resources.paths, task.id).await {
            OperationError::new("cleanup.remove_local_workspace", error).log(
                task.id,
                Some(attempt.attempt.id),
                task.status,
            );
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
        self.checkpoint(TransferCheckpointPoint::LocalCleanupCompleted)
            .await;
        self.checkpoint(TransferCheckpointPoint::BeforeRemoteDelete)
            .await;
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

impl TransferExecutor {
    /// Removes the temporary files of a task that can no longer use them.
    ///
    /// # Errors
    /// Returns [`TransferError`] when the task cannot be read or its files removed.
    pub async fn release_unneeded_workspace(&self, task_id: TaskId) -> Result<(), TransferError> {
        let Some(task) = self.resources.store.task(task_id).await? else {
            return Ok(());
        };
        if local_files_unneeded(&task) {
            remove_task_workspace(&self.resources.paths, task_id).await?;
        }
        Ok(())
    }

    /// Releases the temporary files left by tasks that can no longer use
    /// them, such as after a crash between a failure and its release. Run it
    /// before any stage starts: it does not coordinate with running stages.
    ///
    /// # Errors
    /// Returns [`TransferError`] when the temp root cannot be listed.
    pub async fn release_unneeded_workspaces(&self) -> Result<(), TransferError> {
        for task_id in self.resources.paths.temp_task_ids()? {
            if let Err(error) = self.release_unneeded_workspace(task_id).await {
                tracing::warn!(%task_id, %error, "Unneeded task temporary files were not released");
            }
        }
        Ok(())
    }
}

/// Whether nothing can read a task's Controller temporary files again: it
/// finished, or it failed in a way no retry can resume. A failure that can be
/// resumed keeps them, because download, verification and publication retries
/// reuse the verified output.
pub(crate) fn local_files_unneeded(task: &crate::persistence::TaskRecord) -> bool {
    match task.status {
        TaskStatus::Completed | TaskStatus::Cancelled => true,
        TaskStatus::Failed => task
            .failure
            .as_ref()
            .is_none_or(|failure| Lifecycle::retry_mode(failure) == RetryMode::Blocked),
        TaskStatus::Queued
        | TaskStatus::Reserved
        | TaskStatus::Uploading
        | TaskStatus::Staged
        | TaskStatus::Submitting
        | TaskStatus::Processing
        | TaskStatus::RemoteCompleted
        | TaskStatus::Downloading
        | TaskStatus::Verifying
        | TaskStatus::Publishing
        | TaskStatus::RemoteCleanup => false,
    }
}

pub(crate) async fn remove_task_workspace(
    paths: &crate::paths::PathCapabilities,
    task_id: TaskId,
) -> Result<(), crate::paths::PathError> {
    if let Some(workspace) = paths.temp_workspace(task_id, false)? {
        workspace.remove_all().await?;
    }
    Ok(())
}
