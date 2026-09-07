use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{AttemptId, TaskId, TaskStatus};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskActionRequest {
    pub version: u64,
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
    pub attempt_id: AttemptId,
    pub status: TaskStatus,
}
