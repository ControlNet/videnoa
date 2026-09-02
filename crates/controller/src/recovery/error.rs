#[derive(Debug, thiserror::Error)]
pub enum RecoveryError {
    #[error(transparent)]
    Persistence(#[from] crate::persistence::PersistenceError),
    #[error(transparent)]
    Lifecycle(#[from] crate::lifecycle::LifecycleError),
    #[error(transparent)]
    ClientConfig(#[from] crate::remote::ClientConfigError),
    #[error(transparent)]
    Remote(#[from] crate::remote::VidenoaClientError),
    #[error("durable task is missing its current attempt")]
    MissingAttempt,
    #[error("durable task is missing its assigned worker")]
    MissingWorker,
    #[error("durable attempt is missing remote submission evidence")]
    MissingRemoteEvidence,
    #[error("health retry delay cannot be represented as a timestamp")]
    HealthDelayRange,
    #[error("durable compare-and-swap conflicted during recovery")]
    Conflict,
}
