use chrono::{DateTime, Utc};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

// The mock speaks the real Worker's wire types, so a drift between what the
// Worker writes and what the Controller reads fails to compile here.
pub use videnoa_protocol::{
    CreateJobResponse, ErrorResponse, FileStatResponse, HealthResponse, HealthStatus, JobProgress,
    JobResponse, JobStatus, PresetResponse, RunWorkflowRequest, UploadResponse, WorkflowEntry,
    WorkflowInterface, WorkflowPort, PROTOCOL_VERSION,
};

pub const fn is_active(status: JobStatus) -> bool {
    matches!(status, JobStatus::Queued | JobStatus::Running)
}

pub const fn job_progress(
    current_frame: u64,
    total_frames: Option<u64>,
    fps: f32,
    eta_seconds: Option<f64>,
) -> JobProgress {
    JobProgress {
        current_frame,
        total_frames,
        fps,
        eta_seconds,
    }
}

/// A fixed RFC 3339 timestamp; the mock's clock does not move.
pub fn mock_time(rfc3339: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(rfc3339)
        .expect("mock timestamps are valid RFC 3339")
        .with_timezone(&Utc)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct JobRecord {
    #[serde(flatten)]
    pub response: JobResponse,
}

impl JobRecord {
    pub fn creation_response(&self) -> CreateJobResponse {
        CreateJobResponse {
            id: self.response.id.clone(),
            status: self.response.status,
            created_at: self.response.created_at,
        }
    }
}

#[derive(Clone, Debug)]
pub struct HttpResult<T> {
    pub status: StatusCode,
    pub body: T,
}
