use crate::persistence::PersistenceError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerRegistryErrorCode {
    NotFound,
    Conflict,
    DuplicateName,
    DuplicateApiUrl,
    Referenced,
    CapacityBelowUsage,
    InvalidName,
    Internal,
}

#[derive(Debug, thiserror::Error)]
pub enum WorkerRegistryError {
    #[error("worker was not found")]
    NotFound,
    #[error("worker changed since it was read")]
    Conflict,
    #[error("worker name is already registered")]
    DuplicateName,
    #[error("worker API URL is already registered")]
    DuplicateApiUrl,
    #[error("worker has active or historical task references")]
    Referenced,
    #[error("worker compute slots cannot be lower than durable usage")]
    CapacityBelowUsage,
    #[error("worker name must not be empty")]
    InvalidName,
    #[error("worker registry persistence failed")]
    Persistence(#[from] PersistenceError),
}

impl WorkerRegistryError {
    #[must_use]
    pub const fn code(&self) -> WorkerRegistryErrorCode {
        match self {
            Self::NotFound => WorkerRegistryErrorCode::NotFound,
            Self::Conflict => WorkerRegistryErrorCode::Conflict,
            Self::DuplicateName => WorkerRegistryErrorCode::DuplicateName,
            Self::DuplicateApiUrl => WorkerRegistryErrorCode::DuplicateApiUrl,
            Self::Referenced => WorkerRegistryErrorCode::Referenced,
            Self::CapacityBelowUsage => WorkerRegistryErrorCode::CapacityBelowUsage,
            Self::InvalidName => WorkerRegistryErrorCode::InvalidName,
            Self::Persistence(_) => WorkerRegistryErrorCode::Internal,
        }
    }
}
