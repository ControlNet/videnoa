use crate::domain::TaskStatus;

use super::{CommandKind, Lifecycle, LifecycleError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransitionTarget {
    Status(TaskStatus),
    RetryByFailure,
}

impl Lifecycle {
    /// Returns the single durable target for one command and state.
    ///
    /// # Errors
    /// Returns [`LifecycleError::IllegalCommand`] when the command is not legal.
    pub fn destination(
        status: TaskStatus,
        command: CommandKind,
    ) -> Result<TransitionTarget, LifecycleError> {
        let target = match command {
            CommandKind::Reserve => normal(status, TaskStatus::Queued, TaskStatus::Reserved),
            CommandKind::StartUpload => normal(status, TaskStatus::Reserved, TaskStatus::Uploading),
            CommandKind::FinishUpload => normal(status, TaskStatus::Uploading, TaskStatus::Staged),
            CommandKind::StartSubmission => {
                normal(status, TaskStatus::Staged, TaskStatus::Submitting)
            }
            CommandKind::PersistSubmission => {
                normal(status, TaskStatus::Submitting, TaskStatus::Processing)
            }
            CommandKind::FinishProcessing => {
                normal(status, TaskStatus::Processing, TaskStatus::RemoteCompleted)
            }
            CommandKind::StartDownload => {
                normal(status, TaskStatus::RemoteCompleted, TaskStatus::Downloading)
            }
            CommandKind::FinishDownload => {
                normal(status, TaskStatus::Downloading, TaskStatus::Verifying)
            }
            CommandKind::FinishVerification => {
                normal(status, TaskStatus::Verifying, TaskStatus::Publishing)
            }
            CommandKind::FinishPublication => {
                normal(status, TaskStatus::Publishing, TaskStatus::RemoteCleanup)
            }
            CommandKind::FinishCleanup => {
                normal(status, TaskStatus::RemoteCleanup, TaskStatus::Completed)
            }
            CommandKind::RequestCancellation => cancellation_target(status),
            CommandKind::FinishCancellation => finish_cancellation_target(status),
            CommandKind::Fail => failure_target(status),
            CommandKind::Retry => retry_target(status),
        };
        target.ok_or(LifecycleError::IllegalCommand)
    }
}

fn normal(actual: TaskStatus, expected: TaskStatus, next: TaskStatus) -> Option<TransitionTarget> {
    (actual == expected).then_some(TransitionTarget::Status(next))
}

const fn cancellation_target(status: TaskStatus) -> Option<TransitionTarget> {
    match status {
        TaskStatus::Queued | TaskStatus::Reserved => {
            Some(TransitionTarget::Status(TaskStatus::Cancelled))
        }
        TaskStatus::Uploading
        | TaskStatus::Staged
        | TaskStatus::Submitting
        | TaskStatus::Processing
        | TaskStatus::RemoteCompleted
        | TaskStatus::Downloading
        | TaskStatus::Verifying => Some(TransitionTarget::Status(status)),
        TaskStatus::Publishing
        | TaskStatus::RemoteCleanup
        | TaskStatus::Completed
        | TaskStatus::Failed
        | TaskStatus::Cancelled => None,
    }
}

const fn finish_cancellation_target(status: TaskStatus) -> Option<TransitionTarget> {
    match status {
        TaskStatus::Uploading
        | TaskStatus::Staged
        | TaskStatus::Submitting
        | TaskStatus::Processing
        | TaskStatus::RemoteCompleted
        | TaskStatus::Downloading
        | TaskStatus::Verifying => Some(TransitionTarget::Status(TaskStatus::Cancelled)),
        TaskStatus::Queued
        | TaskStatus::Reserved
        | TaskStatus::Publishing
        | TaskStatus::RemoteCleanup
        | TaskStatus::Completed
        | TaskStatus::Failed
        | TaskStatus::Cancelled => None,
    }
}

const fn failure_target(status: TaskStatus) -> Option<TransitionTarget> {
    match status {
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
        | TaskStatus::RemoteCleanup => Some(TransitionTarget::Status(TaskStatus::Failed)),
        TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled => None,
    }
}

const fn retry_target(status: TaskStatus) -> Option<TransitionTarget> {
    match status {
        TaskStatus::Failed => Some(TransitionTarget::RetryByFailure),
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
        | TaskStatus::RemoteCleanup
        | TaskStatus::Completed
        | TaskStatus::Cancelled => None,
    }
}
