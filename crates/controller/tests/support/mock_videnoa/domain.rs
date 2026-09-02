use std::collections::BTreeMap;

use reqwest::StatusCode;
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
}

impl JobStatus {
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct JobProgress {
    pub current_frame: u64,
    pub total_frames: Option<u64>,
    pub fps: f32,
    pub eta_seconds: Option<f64>,
}

impl JobProgress {
    pub const fn new(
        current_frame: u64,
        total_frames: Option<u64>,
        fps: f32,
        eta_seconds: Option<f64>,
    ) -> Self {
        Self {
            current_frame,
            total_frames,
            fps,
            eta_seconds,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RunRequest {
    #[serde(default)]
    pub workflow_name: Option<String>,
    #[serde(default)]
    pub params: Option<BTreeMap<String, Value>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreateJobResponse {
    pub id: String,
    pub status: JobStatus,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct JobResponse {
    pub id: String,
    pub status: JobStatus,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub progress: Option<JobProgress>,
    pub error: Option<String>,
    pub workflow_name: String,
    pub workflow_source: String,
    pub params: Option<BTreeMap<String, Value>>,
    pub rerun_of_job_id: Option<String>,
    pub duration_ms: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UploadResponse {
    pub path: String,
    pub size: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FileStatResponse {
    pub path: String,
    pub size: u64,
    pub is_file: bool,
    pub is_dir: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkflowPort {
    pub name: String,
    pub port_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_value: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkflowInterface {
    pub inputs: Vec<WorkflowPort>,
    pub outputs: Vec<WorkflowPort>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorkflowEntry {
    pub filename: String,
    pub name: String,
    pub description: String,
    pub workflow: Value,
    pub has_interface: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PresetResponse {
    pub id: String,
    pub name: String,
    pub description: String,
    pub workflow: Value,
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
            created_at: self.response.created_at.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct HttpResult<T> {
    pub status: StatusCode,
    pub body: T,
}
