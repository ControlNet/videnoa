use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
    /// A status this build does not know, sent by a newer Worker. Treat the
    /// job as still active.
    #[serde(other)]
    Unknown,
}

/// Why a queued or running job was asked to stop.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CancelReason {
    /// A client cancelled it (`DELETE /api/jobs/{id}` on an active job).
    User,
    /// The Worker shut down while the job was active.
    Shutdown,
    /// The Worker restarted (or crashed) while the job was active.
    Restart,
    /// A reason this build does not know, sent by a newer Worker.
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct JobProgress {
    pub current_frame: u64,
    pub total_frames: Option<u64>,
    pub fps: f32,
    pub eta_seconds: Option<f64>,
}

/// `GET /api/jobs/{id}`, `GET /api/jobs`, and
/// `GET /api/jobs/by-idempotency-key/{key}`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct JobResponse {
    pub id: String,
    pub status: JobStatus,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub progress: Option<JobProgress>,
    pub error: Option<String>,
    pub workflow_name: String,
    pub workflow_source: String,
    pub params: Option<BTreeMap<String, Value>>,
    pub rerun_of_job_id: Option<String>,
    pub duration_ms: Option<i64>,
    /// Present (and true) once the job was asked to stop; it stays `queued`
    /// or `running` until its executor exits and it becomes `cancelled`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cancel_requested: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel_requested_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel_reason: Option<CancelReason>,
}

/// The receipt of a created (201) or replayed (200) job.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CreateJobResponse {
    pub id: String,
    pub status: JobStatus,
    pub created_at: DateTime<Utc>,
}

/// `POST /api/run` and `POST /api/run/validate`.
///
/// Unknown fields are ignored, so a newer Controller's additive field does
/// not make an older Worker reject the request. Workers up to v0.1.10
/// rejected them with 422.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct RunWorkflowRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<BTreeMap<String, Value>>,
}
