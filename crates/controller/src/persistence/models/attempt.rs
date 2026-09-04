use chrono::{DateTime, Utc};
use uuid::Uuid;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SubmissionOwner(Uuid);

impl SubmissionOwner {
    pub(crate) fn random() -> Self {
        Self(Uuid::new_v4())
    }
}

impl std::fmt::Display for SubmissionOwner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SubmissionClaim {
    pub attempt_id: AttemptId,
    pub expected_version: u64,
    pub owner: SubmissionOwner,
    pub claimed_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SubmissionClaimOutcome {
    Claimed { new_version: u64 },
    Owned,
    Conflict,
}
