use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use videnoa_protocol as wire;
pub use videnoa_protocol::{
    CancelReason, JobProgress, JobStatus, WorkflowInterface, WorkflowPort, PROTOCOL_VERSION,
};

use crate::domain::{RemoteJobId, RemotePath, WorkflowName};

// Worker responses are read through the shared `videnoa-protocol` wire types,
// then given this Controller's typed identifiers. Fields added by a newer
// Worker are ignored and enum values it adds read as `Unknown`, so a Worker
// upgrade cannot break this Controller. Known fields remain strictly typed.

/// `GET /api/health`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(transparent)]
pub struct Health(wire::HealthResponse);

impl Health {
    #[must_use]
    pub fn is_healthy(&self) -> bool {
        self.0.status == wire::HealthStatus::Ok
    }

    /// The Worker's protocol version; 1 for Workers up to v0.1.10.
    #[must_use]
    pub fn protocol(&self) -> u32 {
        self.0.protocol()
    }

    /// Whether this Controller speaks the Worker's protocol: every version
    /// from 1 up to its own [`PROTOCOL_VERSION`].
    #[must_use]
    pub fn protocol_supported(&self) -> bool {
        (wire::LEGACY_PROTOCOL_VERSION..=PROTOCOL_VERSION).contains(&self.protocol())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(from = "wire::WorkflowEntry")]
pub struct Workflow {
    pub filename: WorkflowName,
    pub name: String,
    pub description: String,
    pub workflow: Value,
    pub has_interface: bool,
}

impl From<wire::WorkflowEntry> for Workflow {
    fn from(entry: wire::WorkflowEntry) -> Self {
        Self {
            filename: WorkflowName::new(entry.filename),
            name: entry.name,
            description: entry.description,
            workflow: entry.workflow,
            has_interface: entry.has_interface,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(try_from = "wire::PresetResponse")]
pub struct Preset {
    pub id: WorkflowName,
    pub name: String,
    pub description: String,
    pub workflow: PresetWorkflow,
}

impl TryFrom<wire::PresetResponse> for Preset {
    type Error = serde_json::Error;

    fn try_from(preset: wire::PresetResponse) -> Result<Self, Self::Error> {
        Ok(Self {
            id: WorkflowName::new(preset.id),
            name: preset.name,
            description: preset.description,
            workflow: serde_json::from_value(preset.workflow)?,
        })
    }
}

/// The part of a preset's stored graph this Controller reads.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct PresetWorkflow {
    pub interface: Option<WorkflowInterface>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(try_from = "wire::JobResponse")]
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
    /// The job was asked to stop; it stays queued or running until its
    /// executor exits. Always false on Workers up to v0.1.10.
    pub cancel_requested: bool,
    /// Why the job was asked to stop, when it was.
    pub cancel_reason: Option<CancelReason>,
}

impl TryFrom<wire::JobResponse> for Job {
    type Error = uuid::Error;

    fn try_from(job: wire::JobResponse) -> Result<Self, Self::Error> {
        Ok(Self {
            id: job.id.parse()?,
            status: job.status,
            created_at: job.created_at,
            started_at: job.started_at,
            completed_at: job.completed_at,
            progress: job.progress,
            error: job.error,
            workflow_name: WorkflowName::new(job.workflow_name),
            workflow_source: job.workflow_source,
            params: job.params,
            rerun_of_job_id: job.rerun_of_job_id.as_deref().map(str::parse).transpose()?,
            duration_ms: job.duration_ms,
            cancel_requested: job.cancel_requested,
            cancel_reason: job.cancel_reason,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(try_from = "wire::CreateJobResponse")]
pub struct RunReceipt {
    pub id: RemoteJobId,
    pub status: JobStatus,
    pub created_at: DateTime<Utc>,
}

impl TryFrom<wire::CreateJobResponse> for RunReceipt {
    type Error = uuid::Error;

    fn try_from(receipt: wire::CreateJobResponse) -> Result<Self, Self::Error> {
        Ok(Self {
            id: receipt.id.parse()?,
            status: receipt.status,
            created_at: receipt.created_at,
        })
    }
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
#[serde(from = "wire::UploadResponse")]
pub struct UploadReceipt {
    pub path: RemotePath,
    pub size: u64,
}

impl From<wire::UploadResponse> for UploadReceipt {
    fn from(receipt: wire::UploadResponse) -> Self {
        Self {
            path: RemotePath::new(receipt.path),
            size: receipt.size,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(from = "wire::FileStatResponse")]
pub struct FileStat {
    pub path: RemotePath,
    pub size: u64,
    pub is_file: bool,
    pub is_dir: bool,
}

impl From<wire::FileStatResponse> for FileStat {
    fn from(stat: wire::FileStatResponse) -> Self {
        Self {
            path: RemotePath::new(stat.path),
            size: stat.size,
            is_file: stat.is_file,
            is_dir: stat.is_dir,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DownloadReceipt {
    pub bytes: u64,
}

/// What a Worker says about running a workflow, before any job is created.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RunValidation {
    Valid,
    /// The Worker would reject `POST /api/run` with this bounded reason.
    Invalid {
        reason: String,
    },
    /// The Worker predates `POST /api/run/validate` or does not know the name.
    Unknown,
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
            .insert(
                "added_by_a_newer_worker".to_owned(),
                json!({"nested": [1, 2]}),
            );
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
    fn worker_responses_still_require_known_fields() {
        assert!(
            serde_json::from_value::<RunReceipt>(json!({"id": JOB_ID, "status": "queued"}))
                .is_err()
        );
        assert!(serde_json::from_value::<RunReceipt>(
            json!({"id": "not-a-uuid", "status": "queued", "created_at": CREATED_AT})
        )
        .is_err());
    }

    #[test]
    fn status_values_added_by_newer_workers_read_as_unknown() {
        let receipt: RunReceipt = serde_json::from_value(
            json!({"id": JOB_ID, "status": "paused", "created_at": CREATED_AT}),
        )
        .expect("a new status is readable");
        assert_eq!(receipt.status, JobStatus::Unknown);
        let health: Health =
            serde_json::from_value(json!({"status": "degraded"})).expect("readable");
        assert!(!health.is_healthy());
    }

    #[test]
    fn protocol_is_1_when_missing_and_newer_versions_are_unsupported() {
        let legacy: Health = serde_json::from_value(json!({"status": "ok"})).expect("readable");
        assert_eq!(legacy.protocol(), 1);
        assert!(legacy.protocol_supported());
        let current: Health =
            serde_json::from_value(json!({"status": "ok", "protocol": PROTOCOL_VERSION}))
                .expect("readable");
        assert!(current.protocol_supported());
        let newer: Health =
            serde_json::from_value(json!({"status": "ok", "protocol": PROTOCOL_VERSION + 1}))
                .expect("readable");
        assert!(!newer.protocol_supported());
    }
}
