use crate::lifecycle::LifecycleError;
use crate::persistence::PersistenceError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SchedulerErrorCode {
    Conflict,
    Internal,
}

#[derive(Debug, thiserror::Error)]
pub enum SchedulerError {
    #[error("scheduler state changed since it was read")]
    Conflict,
    #[error("scheduler persistence failed")]
    Persistence(#[from] PersistenceError),
    #[error("durable reservation failed")]
    Lifecycle(#[from] LifecycleError),
}

impl SchedulerError {
    #[must_use]
    pub const fn code(&self) -> SchedulerErrorCode {
        match self {
            Self::Conflict => SchedulerErrorCode::Conflict,
            Self::Persistence(_) | Self::Lifecycle(_) => SchedulerErrorCode::Internal,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("transfer concurrency limits must be greater than zero")]
pub struct TransferLimitError;
