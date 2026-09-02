use chrono::{DateTime, Utc};

use crate::domain::{ComputeSlots, WorkerApiUrl, WorkerCapabilities, WorkerId, WorkerName};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewWorker {
    pub id: WorkerId,
    pub name: WorkerName,
    pub api_url: WorkerApiUrl,
    pub enabled: bool,
    pub online: bool,
    pub compute_slots: ComputeSlots,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WorkerRecord {
    pub id: WorkerId,
    pub version: u64,
    pub name: WorkerName,
    pub api_url: WorkerApiUrl,
    pub enabled: bool,
    pub online: bool,
    pub compute_slots: ComputeSlots,
    pub capabilities: WorkerCapabilities,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub last_assigned_at: Option<DateTime<Utc>>,
    pub health_retry_count: u32,
    pub next_health_check_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkerUpdate {
    pub id: WorkerId,
    pub expected_version: u64,
    pub name: WorkerName,
    pub api_url: WorkerApiUrl,
    pub enabled: bool,
    pub compute_slots: ComputeSlots,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerUpdateOutcome {
    Applied { new_version: u64 },
    Conflict,
    CapacityBelowUsage,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WorkerHealthUpdate {
    pub id: WorkerId,
    pub expected_version: u64,
    pub online: bool,
    pub capabilities: WorkerCapabilities,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub health_retry_count: u32,
    pub next_health_check_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerIdentityConflict {
    Name,
    ApiUrl,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerDeleteOutcome {
    Deleted,
    NotFound,
    Conflict,
    Referenced,
}
