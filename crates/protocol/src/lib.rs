//! Wire types of the HTTP API a Videnoa Worker serves to a Videnoa Controller.
//!
//! The Worker serializes these types, and the Controller and its test mock
//! read them, so all three compile against one definition.
//!
//! Rules for changing them (see `docs/controller.md`, "Worker API
//! compatibility"):
//!
//! - JSON stays byte-compatible for existing fields. A new response field is
//!   additive: optional on read and, where an older Worker cannot send it,
//!   defaulted.
//! - Response enums the Controller reads end in an `Unknown` variant, so a
//!   value added by a newer Worker is readable instead of failing the whole
//!   response.
//! - Request types do not reject unknown fields, so an older Worker accepts a
//!   request that carries a field it does not know.
//! - [`PROTOCOL_VERSION`] changes only when a Controller must understand the
//!   change to work with the Worker at all.

mod error;
mod files;
mod health;
mod jobs;
mod workflows;

pub use error::{ErrorCode, ErrorResponse};
pub use files::{FileStatResponse, UploadResponse};
pub use health::{HealthResponse, HealthStatus};
pub use jobs::{
    CancelReason, CreateJobResponse, JobProgress, JobResponse, JobStatus, RunWorkflowRequest,
};
pub use workflows::{PresetResponse, WorkflowEntry, WorkflowInterface, WorkflowPort};

/// The protocol this build speaks, reported in `/api/health`.
///
/// Version 1 is every Worker up to v0.1.10, whose health has no `protocol`
/// field. Version 2 adds typed error codes, the cancel fields of a job, and
/// `GET /api/jobs/by-idempotency-key/{key}`.
pub const PROTOCOL_VERSION: u32 = 2;

/// The protocol of a Worker whose health does not report one.
pub const LEGACY_PROTOCOL_VERSION: u32 = 1;
