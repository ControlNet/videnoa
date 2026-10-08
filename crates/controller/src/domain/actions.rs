use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{AttemptId, TaskId, TaskStatus, WorkerId};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskActionRequest {
    pub version: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RetryTaskRequest {
    pub version: u64,
    /// Queues the retry for this Worker instead of the failed attempt's Worker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<WorkerId>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CancelTaskResponse {
    pub task_id: TaskId,
    pub status: TaskStatus,
    pub cancel_requested_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RetryTaskResponse {
    pub task_id: TaskId,
    /// The resumed attempt, or `None` when the task went back to the queue.
    pub attempt_id: Option<AttemptId>,
    pub status: TaskStatus,
}
