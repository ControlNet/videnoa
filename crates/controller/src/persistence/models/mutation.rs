use chrono::{DateTime, Utc};

use crate::domain::{AttemptId, FailureInfo, RetryMetadata, TaskId, TaskProgress};

use super::PublicationEvidence;

#[derive(Clone, Debug, PartialEq)]
pub struct TaskProgressUpdate {
    pub task_id: TaskId,
    pub expected_version: u64,
    pub progress: TaskProgress,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskFailureUpdate {
    pub task_id: TaskId,
    pub expected_version: u64,
    pub failure: Option<FailureInfo>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskRetryUpdate {
    pub task_id: TaskId,
    pub expected_version: u64,
    pub retry: RetryMetadata,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicationUpdate {
    pub task_id: TaskId,
    pub expected_version: u64,
    pub evidence: PublicationEvidence,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AttemptProgressUpdate {
    pub attempt_id: AttemptId,
    pub expected_version: u64,
    pub progress: TaskProgress,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttemptFailureUpdate {
    pub attempt_id: AttemptId,
    pub expected_version: u64,
    pub failure: Option<FailureInfo>,
    pub retry: RetryMetadata,
    pub updated_at: DateTime<Utc>,
}
