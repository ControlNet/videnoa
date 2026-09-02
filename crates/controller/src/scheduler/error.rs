use crate::lifecycle::{JitterRangeError, LifecycleError};
use crate::paths::PathError;
use crate::persistence::PersistenceError;
use crate::remote::{ClientConfigError, VidenoaClientError};

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

#[derive(Debug, thiserror::Error)]
pub enum TransferError {
    #[error("transfer state changed since it was read")]
    Conflict,
    #[error("transfer concurrency is saturated")]
    Busy,
    #[error("transfer retry deadline has not elapsed")]
    RetryNotDue,
    #[error("durable transfer state is incomplete")]
    MissingEvidence,
    #[error("transfer retry time is outside the supported range")]
    TimeRange,
    #[error("transfer retry jitter is outside the supported range")]
    Jitter(#[from] JitterRangeError),
    #[error("transfer persistence failed")]
    Persistence(#[from] PersistenceError),
    #[error("transfer lifecycle transition failed")]
    Lifecycle(#[from] LifecycleError),
    #[error("local transfer path failed")]
    Path(#[from] PathError),
    #[error("remote transfer failed")]
    Remote(#[from] VidenoaClientError),
    #[error("remote client configuration failed")]
    ClientConfig(#[from] ClientConfigError),
    #[error("local transfer I/O failed")]
    Io(#[from] std::io::Error),
}
