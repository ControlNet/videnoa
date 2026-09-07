use crate::domain::ApiErrorCode;
use crate::persistence::PersistenceError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleErrorCode {
    IllegalCommand,
    Conflict,
    RemoteStateAmbiguous,
    PublicationAmbiguous,
    Internal,
}

#[derive(Debug, thiserror::Error)]
pub enum LifecycleError {
    #[error("lifecycle command is illegal for the durable state")]
    IllegalCommand,
    #[error("lifecycle command conflicts with newer durable state")]
    Conflict,
    #[error("cancellation intent is required before cancellation completion")]
    CancellationIntentRequired,
    #[error("submitting cancellation requires keyed submission reconciliation")]
    SubmissionReconciliationRequired,
    #[error("task and attempt lifecycle snapshots disagree")]
    AttemptMismatch,
    #[error("a lifecycle attempt is required for this command")]
    AttemptRequired,
    #[error("remote state is ambiguous")]
    RemoteStateAmbiguous,
    #[error("publication state is ambiguous")]
    PublicationAmbiguous,
    #[error("remote terminal evidence does not match the durable attempt")]
    RemoteEvidenceMismatch,
    #[error("remote workspace cleanup evidence does not match the durable task")]
    WorkspaceEvidenceMismatch,
    #[error("lifecycle persistence failed")]
    Persistence(#[from] PersistenceError),
}

impl LifecycleError {
    #[must_use]
    pub const fn code(&self) -> LifecycleErrorCode {
        match self {
            Self::IllegalCommand
            | Self::CancellationIntentRequired
            | Self::SubmissionReconciliationRequired => LifecycleErrorCode::IllegalCommand,
            Self::Conflict
            | Self::AttemptMismatch
            | Self::AttemptRequired
            | Self::RemoteEvidenceMismatch
            | Self::WorkspaceEvidenceMismatch => LifecycleErrorCode::Conflict,
            Self::RemoteStateAmbiguous => LifecycleErrorCode::RemoteStateAmbiguous,
            Self::PublicationAmbiguous => LifecycleErrorCode::PublicationAmbiguous,
            Self::Persistence(_) => LifecycleErrorCode::Internal,
        }
    }

    #[must_use]
    pub const fn api_code(&self) -> ApiErrorCode {
        match self.code() {
            LifecycleErrorCode::IllegalCommand | LifecycleErrorCode::Conflict => {
                ApiErrorCode::Conflict
            }
            LifecycleErrorCode::RemoteStateAmbiguous => ApiErrorCode::RemoteStateAmbiguous,
            LifecycleErrorCode::PublicationAmbiguous => ApiErrorCode::PublicationAmbiguous,
            LifecycleErrorCode::Internal => ApiErrorCode::InternalError,
        }
    }

    #[must_use]
    pub const fn retryable(&self) -> bool {
        match self.code() {
            LifecycleErrorCode::IllegalCommand
            | LifecycleErrorCode::Conflict
            | LifecycleErrorCode::RemoteStateAmbiguous
            | LifecycleErrorCode::PublicationAmbiguous
            | LifecycleErrorCode::Internal => false,
        }
    }
}
