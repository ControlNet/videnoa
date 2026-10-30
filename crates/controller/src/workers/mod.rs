#[path = "error.rs"]
mod failures;
mod health;
mod service;

pub use failures::{WorkerRegistryError, WorkerRegistryErrorCode};
pub(crate) use health::AUTHENTICATION_FAILED;
pub use health::{WorkerHealthError, WorkerHealthService};
pub use service::WorkerRegistry;
