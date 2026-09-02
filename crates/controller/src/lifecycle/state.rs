use crate::domain::TaskStatus;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandKind {
    Reserve,
    StartUpload,
    FinishUpload,
    StartSubmission,
    PersistSubmission,
    FinishProcessing,
    StartDownload,
    FinishDownload,
    FinishVerification,
    FinishPublication,
    FinishCleanup,
    RequestCancellation,
    FinishCancellation,
    Fail,
    Retry,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryAction {
    AwaitReservation,
    BeginUpload,
    ReconcileUpload,
    BeginSubmission,
    ReconcileSubmission,
    PollProcessing,
    BeginDownload,
    RestartDownload,
    Reverify,
    ReconcilePublication,
    RetryCleanup,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Lifecycle;

impl Lifecycle {
    #[must_use]
    pub const fn commands(status: TaskStatus) -> &'static [CommandKind] {
        match status {
            TaskStatus::Queued => &[
                CommandKind::Reserve,
                CommandKind::RequestCancellation,
                CommandKind::Fail,
            ],
            TaskStatus::Reserved => &[
                CommandKind::StartUpload,
                CommandKind::RequestCancellation,
                CommandKind::Fail,
            ],
            TaskStatus::Uploading => &[
                CommandKind::FinishUpload,
                CommandKind::RequestCancellation,
                CommandKind::FinishCancellation,
                CommandKind::Fail,
            ],
            TaskStatus::Staged => &[
                CommandKind::StartSubmission,
                CommandKind::RequestCancellation,
                CommandKind::FinishCancellation,
                CommandKind::Fail,
            ],
            TaskStatus::Submitting => &[
                CommandKind::PersistSubmission,
                CommandKind::RequestCancellation,
                CommandKind::FinishCancellation,
                CommandKind::Fail,
            ],
            TaskStatus::Processing => &[
                CommandKind::FinishProcessing,
                CommandKind::RequestCancellation,
                CommandKind::FinishCancellation,
                CommandKind::Fail,
            ],
            TaskStatus::RemoteCompleted => &[
                CommandKind::StartDownload,
                CommandKind::RequestCancellation,
                CommandKind::FinishCancellation,
                CommandKind::Fail,
            ],
            TaskStatus::Downloading => &[
                CommandKind::FinishDownload,
                CommandKind::RequestCancellation,
                CommandKind::FinishCancellation,
                CommandKind::Fail,
            ],
            TaskStatus::Verifying => &[
                CommandKind::FinishVerification,
                CommandKind::RequestCancellation,
                CommandKind::FinishCancellation,
                CommandKind::Fail,
            ],
            TaskStatus::Publishing => &[CommandKind::FinishPublication, CommandKind::Fail],
            TaskStatus::RemoteCleanup => &[CommandKind::FinishCleanup, CommandKind::Fail],
            TaskStatus::Completed | TaskStatus::Cancelled => &[],
            TaskStatus::Failed => &[CommandKind::Retry],
        }
    }

    #[must_use]
    pub const fn recovery(status: TaskStatus) -> RecoveryAction {
        match status {
            TaskStatus::Queued => RecoveryAction::AwaitReservation,
            TaskStatus::Reserved => RecoveryAction::BeginUpload,
            TaskStatus::Uploading => RecoveryAction::ReconcileUpload,
            TaskStatus::Staged => RecoveryAction::BeginSubmission,
            TaskStatus::Submitting => RecoveryAction::ReconcileSubmission,
            TaskStatus::Processing => RecoveryAction::PollProcessing,
            TaskStatus::RemoteCompleted => RecoveryAction::BeginDownload,
            TaskStatus::Downloading => RecoveryAction::RestartDownload,
            TaskStatus::Verifying => RecoveryAction::Reverify,
            TaskStatus::Publishing => RecoveryAction::ReconcilePublication,
            TaskStatus::RemoteCleanup => RecoveryAction::RetryCleanup,
            TaskStatus::Completed => RecoveryAction::Completed,
            TaskStatus::Failed => RecoveryAction::Failed,
            TaskStatus::Cancelled => RecoveryAction::Cancelled,
        }
    }
}
