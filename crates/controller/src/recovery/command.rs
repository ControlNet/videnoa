use crate::lifecycle::RecoveryAction;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryCommandKind {
    AwaitReservation,
    Upload,
    Submit,
    Poll,
    Download,
    Verify,
    Publish,
    Cleanup,
    Terminal,
}

impl RecoveryCommandKind {
    #[must_use]
    pub const fn for_action(action: RecoveryAction) -> Self {
        match action {
            RecoveryAction::AwaitReservation => Self::AwaitReservation,
            RecoveryAction::BeginUpload | RecoveryAction::ReconcileUpload => Self::Upload,
            RecoveryAction::BeginSubmission | RecoveryAction::ReconcileSubmission => Self::Submit,
            RecoveryAction::PollProcessing => Self::Poll,
            RecoveryAction::BeginDownload | RecoveryAction::RestartDownload => Self::Download,
            RecoveryAction::Reverify => Self::Verify,
            RecoveryAction::ReconcilePublication => Self::Publish,
            RecoveryAction::RetryCleanup => Self::Cleanup,
            RecoveryAction::Completed | RecoveryAction::Failed | RecoveryAction::Cancelled => {
                Self::Terminal
            }
        }
    }
}
