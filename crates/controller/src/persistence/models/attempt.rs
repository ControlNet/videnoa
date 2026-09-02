use chrono::{DateTime, Utc};

use crate::domain::{AttemptId, RemoteJobId, RemotePath, TaskAttempt};

#[derive(Clone, Debug, PartialEq)]
pub struct AttemptRecord {
    pub attempt: TaskAttempt,
    pub version: u64,
    pub updated_at: DateTime<Utc>,
    pub submitted_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttemptRemoteUpdate {
    pub attempt_id: AttemptId,
    pub expected_version: u64,
    pub remote_job_id: RemoteJobId,
    pub remote_input_path: RemotePath,
    pub remote_output_path: RemotePath,
    pub submitted_at: DateTime<Utc>,
}
