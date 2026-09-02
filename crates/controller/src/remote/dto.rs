use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::{RemoteJobId, RemotePath, WorkflowName};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum HealthStatus {
    Ok,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Health {
    status: HealthStatus,
}

impl Health {
    #[must_use]
    pub const fn is_healthy(self) -> bool {
        match self.status {
            HealthStatus::Ok => true,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Workflow {
    pub filename: WorkflowName,
    pub name: String,
    pub description: String,
    pub workflow: Value,
    pub has_interface: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    pub id: WorkflowName,
    pub name: String,
    pub description: String,
    pub workflow: Value,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkflowPort {
    pub name: String,
    pub port_type: String,
    pub default_value: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkflowInterface {
    pub inputs: Vec<WorkflowPort>,
    pub outputs: Vec<WorkflowPort>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct JobProgress {
    pub current_frame: u64,
    pub total_frames: Option<u64>,
    pub fps: f32,
    pub eta_seconds: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Job {
    pub id: RemoteJobId,
    pub status: JobStatus,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub progress: Option<JobProgress>,
    pub error: Option<String>,
    pub workflow_name: WorkflowName,
    pub workflow_source: String,
    pub params: Option<BTreeMap<String, Value>>,
    pub rerun_of_job_id: Option<RemoteJobId>,
    pub duration_ms: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RunReceipt {
    pub id: RemoteJobId,
    pub status: JobStatus,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunOutcome {
    Created,
    Replayed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunSubmission {
    pub outcome: RunOutcome,
    pub receipt: RunReceipt,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct UploadReceipt {
    pub path: RemotePath,
    pub size: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FileStat {
    pub path: RemotePath,
    pub size: u64,
    pub is_file: bool,
    pub is_dir: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DownloadReceipt {
    pub bytes: u64,
}

#[derive(Serialize)]
pub(crate) struct RunRequest<'a> {
    pub workflow_name: &'a WorkflowName,
    pub params: &'a BTreeMap<String, Value>,
}
