use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{
    ComputeSlots, TaskProgress, WorkerApiUrl, WorkerId, WorkerName, WorkflowKind, WorkflowName,
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowSummary {
    pub name: WorkflowName,
    pub kind: WorkflowKind,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerCapabilities {
    pub workflows: Vec<WorkflowSummary>,
    pub refreshed_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerCapacity {
    pub used_slots: u16,
    pub available_slots: u16,
    pub assigned_tasks: u32,
    pub staged_tasks: u32,
    pub processing_tasks: u32,
    pub active_uploads: u16,
    pub active_downloads: u16,
    pub progress: Option<TaskProgress>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerSummary {
    pub id: WorkerId,
    pub version: u64,
    pub name: WorkerName,
    pub api_url: WorkerApiUrl,
    pub enabled: bool,
    pub online: bool,
    pub compute_slots: ComputeSlots,
    pub capabilities: WorkerCapabilities,
    pub capacity: WorkerCapacity,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub last_assigned_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_error: Option<String>,
    #[serde(default)]
    pub has_password: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerCreateRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<super::SecretString>,
    pub name: WorkerName,
    pub api_url: WorkerApiUrl,
    pub enabled: bool,
    pub compute_slots: ComputeSlots,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerUpdateRequest {
    #[serde(
        default,
        deserialize_with = "password_change",
        skip_serializing_if = "Option::is_none"
    )]
    pub password: Option<Option<super::SecretString>>,
    pub version: u64,
    pub name: WorkerName,
    pub api_url: WorkerApiUrl,
    pub enabled: bool,
    pub compute_slots: ComputeSlots,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerDeleteResponse {
    pub worker_id: WorkerId,
    pub deleted: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerListResponse {
    pub items: Vec<WorkerSummary>,
    pub total: u64,
}

// Missing preserves the credential; JSON null explicitly removes it.
#[allow(clippy::option_option)]
fn password_change<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Option<super::SecretString>>, D::Error> {
    Option::<super::SecretString>::deserialize(deserializer).map(Some)
}
