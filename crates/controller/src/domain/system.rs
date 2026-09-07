use serde::{Deserialize, Serialize};

use super::{
    HealthStatus, ReadinessStatus, SchedulerStatus, SseEventId, SseEventKind, TaskStatus,
    TaskSummary, WorkerSummary,
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HealthResponse {
    pub status: HealthStatus,
}

/// Build identity of the running Controller binary.
///
/// The frontend is embedded in this binary, so a bundle-side constant would
/// only restate what the build already knows -- and it would be wrong in
/// development, where a proxied dev bundle may be talking to any build. The
/// server is therefore the authority for what it is.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AboutResponse {
    pub name: String,
    pub version: String,
    pub source_url: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReadinessCheck {
    pub name: String,
    pub ready: bool,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReadinessResponse {
    pub status: ReadinessStatus,
    pub checks: Vec<ReadinessCheck>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskStatusCount {
    pub status: TaskStatus,
    pub count: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskStatusCountsResponse {
    pub items: Vec<TaskStatusCount>,
    pub total: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum SseEvent {
    TaskUpdated {
        event_id: SseEventId,
        task: TaskSummary,
    },
    WorkerUpdated {
        event_id: SseEventId,
        worker: WorkerSummary,
    },
    SchedulerUpdated {
        event_id: SseEventId,
        scheduler: SchedulerStatus,
    },
}

impl SseEvent {
    #[must_use]
    pub const fn kind(&self) -> SseEventKind {
        match self {
            Self::TaskUpdated { .. } => SseEventKind::TaskUpdated,
            Self::WorkerUpdated { .. } => SseEventKind::WorkerUpdated,
            Self::SchedulerUpdated { .. } => SseEventKind::SchedulerUpdated,
        }
    }
}
