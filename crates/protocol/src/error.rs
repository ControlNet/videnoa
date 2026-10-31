use serde::{Deserialize, Serialize};

/// The body of a Worker error response. `error` is a human-readable reason;
/// `code`, when present, says what failed in a way a client can act on.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ErrorResponse {
    pub error: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<ErrorCode>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The `Idempotency-Key` header is malformed.
    InvalidIdempotencyKey,
    /// The idempotency key belongs to a submission with other content.
    IdempotencyConflict,
    /// No job was created under this idempotency key, or no job has this id.
    JobNotFound,
    /// `workflow_name` is missing or is not a plain run name.
    WorkflowNameInvalid,
    /// No saved workflow or preset has this run name.
    WorkflowNotFound,
    /// The workflow file exists but could not be read.
    WorkflowUnreadable,
    /// The workflow is not valid JSON or fails validation.
    WorkflowInvalid,
    /// The Worker's job store is not available, so it cannot accept jobs.
    JobStoreUnavailable,
    /// The Worker holds as many queued and running jobs as it accepts; retry
    /// after some finish.
    QueueFull,
    /// The Worker is shutting down; retry after it restarts.
    ShuttingDown,
    /// A Worker without a password was reached by a host name it does not
    /// trust (`server.allowed_hosts`).
    HostNotAllowed,
    /// Saving a workflow would replace an existing file.
    WorkflowExists,
    /// A saved workflow would use a preset's run name.
    WorkflowNameIsPreset,
    /// The config changed since it was read (`If-Match` mismatch).
    ConfigChanged,
    /// A code this build does not know, sent by a newer Worker.
    #[serde(other)]
    Unknown,
}
