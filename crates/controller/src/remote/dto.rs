use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::{RemoteJobId, RemotePath, WorkflowName};

// Worker responses are read tolerantly: fields added by a newer Worker are
// ignored, so a Worker upgrade cannot break this Controller. Known fields and
// enum values remain strictly typed.

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum HealthStatus {
    Ok,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
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
pub struct Workflow {
    pub filename: WorkflowName,
    pub name: String,
    pub description: String,
    pub workflow: Value,
    pub has_interface: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Preset {
    pub id: WorkflowName,
    pub name: String,
    pub description: String,
    pub workflow: PresetWorkflow,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct PresetWorkflow {
    pub interface: Option<WorkflowInterface>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct WorkflowPort {
    pub name: String,
    pub port_type: String,
    pub default_value: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
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
pub struct JobProgress {
    pub current_frame: u64,
    pub total_frames: Option<u64>,
    pub fps: f32,
    pub eta_seconds: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
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
pub struct UploadReceipt {
    pub path: RemotePath,
    pub size: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
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

#[cfg(test)]
mod tests {
    use serde::de::DeserializeOwned;
    use serde_json::{json, Value};

    use super::*;

    const JOB_ID: &str = "00000000-0000-4000-8000-000000000001";
    const CREATED_AT: &str = "2026-10-02T00:00:00Z";

    fn with_extra_field(mut value: Value) -> Value {
        value
            .as_object_mut()
            .expect("fixtures are JSON objects")
            .insert("added_by_a_newer_worker".to_owned(), json!({"nested": [1, 2]}));
        value
    }

    fn parses_with_extra_field<T: DeserializeOwned>(value: Value) {
        serde_json::from_value::<T>(value.clone()).expect("current fields parse");
        serde_json::from_value::<T>(with_extra_field(value))
            .expect("an unknown field from a newer Worker is ignored");
    }

    fn interface() -> Value {
        json!({
            "inputs": [{"name": "input", "port_type": "Path", "default_value": null}],
            "outputs": [{"name": "output", "port_type": "Path", "default_value": null}],
        })
    }

    #[test]
    fn worker_responses_ignore_fields_added_by_newer_workers() {
        parses_with_extra_field::<Health>(json!({"status": "ok"}));
        parses_with_extra_field::<Workflow>(json!({
            "filename": "anime-2x.json",
            "name": "Anime 2x",
            "description": "",
            "workflow": {},
            "has_interface": true,
        }));
        parses_with_extra_field::<Preset>(json!({
            "id": "anime-2x",
            "name": "Anime 2x",
            "description": "",
            "workflow": {"interface": interface()},
        }));
        parses_with_extra_field::<WorkflowInterface>(interface());
        parses_with_extra_field::<WorkflowPort>(
            json!({"name": "input", "port_type": "Path", "default_value": null}),
        );
        parses_with_extra_field::<JobProgress>(
            json!({"current_frame": 1, "total_frames": 2, "fps": 1.5, "eta_seconds": 1.0}),
        );
        parses_with_extra_field::<Job>(json!({
            "id": JOB_ID,
            "status": "running",
            "created_at": CREATED_AT,
            "started_at": null,
            "completed_at": null,
            "progress": null,
            "error": null,
            "workflow_name": "anime-2x",
            "workflow_source": "preset",
            "params": null,
            "rerun_of_job_id": null,
            "duration_ms": null,
        }));
        parses_with_extra_field::<RunReceipt>(
            json!({"id": JOB_ID, "status": "queued", "created_at": CREATED_AT}),
        );
        parses_with_extra_field::<UploadReceipt>(json!({"path": "task/input.mkv", "size": 3}));
        parses_with_extra_field::<FileStat>(
            json!({"path": "task/input.mkv", "size": 3, "is_file": true, "is_dir": false}),
        );
    }

    #[test]
    fn worker_responses_still_require_known_fields_and_statuses() {
        assert!(serde_json::from_value::<RunReceipt>(json!({"id": JOB_ID, "status": "queued"})).is_err());
        assert!(serde_json::from_value::<RunReceipt>(
            json!({"id": JOB_ID, "status": "paused", "created_at": CREATED_AT})
        )
        .is_err());
        assert!(serde_json::from_value::<Health>(json!({"status": "degraded"})).is_err());
    }
}
