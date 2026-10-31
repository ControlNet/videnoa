mod iroh;
pub use iroh::{configure_iroh, shutdown_iroh};
#[path = "cache.rs"]
mod capability_store;
mod catalog;
mod config;
#[path = "client.rs"]
mod connector;
mod dto;
mod jobs;
mod paths;
#[path = "error.rs"]
mod request_failure;
mod transfer;
mod transport;
mod upload;
#[path = "compatibility.rs"]
mod workflow_eligibility;

pub use capability_store::{
    CacheInvalidation, CapabilityCache, CompatibilityEvidence, MonotonicClock, SystemClock,
};
pub use config::{PayloadLimits, RemoteTimeouts};
pub use connector::VidenoaClient;
pub use dto::{
    CancelReason, DownloadReceipt, FileStat, Health, Job, JobProgress, JobStatus, Preset,
    PresetWorkflow, RunOutcome, RunReceipt, RunSubmission, RunValidation, UploadReceipt, Workflow,
    WorkflowInterface, WorkflowPort, PROTOCOL_VERSION,
};
pub use paths::{sibling_output_path, FileApiPath};
pub use request_failure::{ClientConfigError, VidenoaClientError};
pub use videnoa_protocol::ErrorCode;
pub use workflow_eligibility::{Compatibility, CompatibilityCatalog, CompatibilityEntry};
