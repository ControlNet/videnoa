use crate::lifecycle::RecoveryAction;

use super::RecoveryCommandKind;

pub(super) const fn local_first_command(action: RecoveryAction) -> Option<RecoveryCommandKind> {
    match action {
        RecoveryAction::Reverify => Some(RecoveryCommandKind::Verify),
        RecoveryAction::ReconcilePublication => Some(RecoveryCommandKind::Publish),
        RecoveryAction::RetryCleanup => Some(RecoveryCommandKind::Cleanup),
        RecoveryAction::AwaitReservation
        | RecoveryAction::BeginUpload
        | RecoveryAction::ReconcileUpload
        | RecoveryAction::BeginSubmission
        | RecoveryAction::ReconcileSubmission
        | RecoveryAction::PollProcessing
        | RecoveryAction::BeginDownload
        | RecoveryAction::RestartDownload
        | RecoveryAction::Completed
        | RecoveryAction::Failed
        | RecoveryAction::Cancelled => None,
    }
}
