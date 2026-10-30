use crate::domain::{TaskAction, TaskStatus};
use crate::persistence::TaskRecord;

use super::{Lifecycle, ResumeStage, RetryMode};

impl Lifecycle {
    /// The operator actions `task` accepts now, in [`TaskAction::ALL`] order.
    /// The action handlers enforce the same rules; clients use this list to
    /// decide which actions to offer.
    #[must_use]
    pub fn allowed_actions(task: &TaskRecord) -> Vec<TaskAction> {
        TaskAction::ALL
            .iter()
            .copied()
            .filter(|action| allows(task, *action))
            .collect()
    }
}

fn allows(task: &TaskRecord, action: TaskAction) -> bool {
    let cancel_requested = task.cancel_requested_at.is_some();
    match action {
        TaskAction::Cancel => !cancel_requested && Lifecycle::cancellation(task.status).is_ok(),
        TaskAction::Abandon => cancel_requested && Lifecycle::abandonable(task.status),
        TaskAction::Retry => retry_mode(task).is_some_and(|mode| mode != RetryMode::Blocked),
        TaskAction::RetryOnAnotherWorker => matches!(
            retry_mode(task),
            Some(
                RetryMode::NewProcessingAttempt
                    | RetryMode::Resume(ResumeStage::Uploading | ResumeStage::Staged)
            )
        ),
        TaskAction::Discard => task.status == TaskStatus::Failed && task.discarded_at.is_none(),
    }
}

/// How Retry would resume the task, or `None` when Retry cannot touch it: it
/// did not fail, was discarded, or its Worker was deleted (deleting a Worker
/// detaches its tasks and their attempts together).
fn retry_mode(task: &TaskRecord) -> Option<RetryMode> {
    if task.status != TaskStatus::Failed || task.discarded_at.is_some() || task.worker_id.is_none()
    {
        return None;
    }
    task.failure.as_ref().map(Lifecycle::retry_mode)
}
