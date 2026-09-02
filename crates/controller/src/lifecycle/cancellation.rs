use chrono::{DateTime, Utc};

use crate::domain::TaskStatus;
use crate::persistence::{AttemptRecord, TaskRecord};

use super::service::{applied, attempt_cas};
use super::{
    CancellationWrite, CommandKind, CommittedCommand, DurableAction, Lifecycle, LifecycleError,
    LifecycleService, PairedTransition, TransitionTarget,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CancelAction {
    CancelLocal,
    AbortUploadAndClean,
    CleanStaged,
    ReconcileSubmission,
    CancelRemoteAndClean,
    AbortDownstreamAndClean,
}

impl Lifecycle {
    /// Classifies the durable cancellation action for one lifecycle state.
    ///
    /// # Errors
    /// Returns a conflict for publication, cleanup, and terminal states.
    pub const fn cancellation(status: TaskStatus) -> Result<CancelAction, LifecycleError> {
        match status {
            TaskStatus::Queued | TaskStatus::Reserved => Ok(CancelAction::CancelLocal),
            TaskStatus::Uploading => Ok(CancelAction::AbortUploadAndClean),
            TaskStatus::Staged => Ok(CancelAction::CleanStaged),
            TaskStatus::Submitting => Ok(CancelAction::ReconcileSubmission),
            TaskStatus::Processing => Ok(CancelAction::CancelRemoteAndClean),
            TaskStatus::RemoteCompleted | TaskStatus::Downloading | TaskStatus::Verifying => {
                Ok(CancelAction::AbortDownstreamAndClean)
            }
            TaskStatus::Publishing
            | TaskStatus::RemoteCleanup
            | TaskStatus::Completed
            | TaskStatus::Failed
            | TaskStatus::Cancelled => Err(LifecycleError::Conflict),
        }
    }
}

impl LifecycleService {
    /// Persists cancellation intent before returning the required cancellation action.
    ///
    /// # Errors
    /// Returns a conflict for late cancellation, mismatched snapshots, or stale versions.
    pub async fn request_cancellation(
        &self,
        task: &TaskRecord,
        attempt: Option<&AttemptRecord>,
        requested_at: DateTime<Utc>,
    ) -> Result<CommittedCommand, LifecycleError> {
        let action = Lifecycle::cancellation(task.status)?;
        let target = Lifecycle::destination(task.status, CommandKind::RequestCancellation)?;
        let TransitionTarget::Status(next_status) = target else {
            return Err(LifecycleError::IllegalCommand);
        };
        let attempt = match attempt {
            Some(attempt) => Some(attempt_cas(task, attempt)?),
            None if task.status == TaskStatus::Queued => None,
            None => return Err(LifecycleError::AttemptRequired),
        };
        let write = CancellationWrite {
            task_id: task.id,
            task_version: task.version,
            from: task.status,
            attempt,
            requested_at,
            immediate: next_status == TaskStatus::Cancelled,
        };
        let version = applied(self.store().request_lifecycle_cancellation(&write).await?)?;
        Ok(CommittedCommand::new(
            next_status,
            version,
            DurableAction::Cancel(action),
        ))
    }

    /// Commits terminal cancellation after the state-specific cleanup action converges.
    ///
    /// # Errors
    /// Returns an error when intent is absent, snapshots disagree, or CAS conflicts.
    pub async fn finish_cancellation(
        &self,
        task: &TaskRecord,
        attempt: &AttemptRecord,
        occurred_at: DateTime<Utc>,
    ) -> Result<CommittedCommand, LifecycleError> {
        if task.cancel_requested_at.is_none() {
            return Err(LifecycleError::CancellationIntentRequired);
        }
        let target = Lifecycle::destination(task.status, CommandKind::FinishCancellation)?;
        let TransitionTarget::Status(next_status) = target else {
            return Err(LifecycleError::IllegalCommand);
        };
        let write = PairedTransition {
            task_id: task.id,
            task_version: task.version,
            from: task.status,
            to: next_status,
            attempt: attempt_cas(task, attempt)?,
            occurred_at,
            submission: None,
        };
        let version = applied(self.store().apply_lifecycle_transition(&write).await?)?;
        Ok(CommittedCommand::new(
            next_status,
            version,
            DurableAction::None,
        ))
    }
}
