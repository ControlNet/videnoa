use chrono::{DateTime, Utc};

use crate::domain::{
    AttemptId, FailureInfo, InputExtension, OutputExtension, RetryMetadata, SubmissionKey,
    TaskCreateRequest, TaskId, TaskProgress, TaskStatus, WorkerId,
};

use super::{InputIdentity, Sha256Digest};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewTask {
    pub id: TaskId,
    pub request: TaskCreateRequest,
    pub input_extension: InputExtension,
    pub output_extension: OutputExtension,
    pub input_size: u64,
    pub input_mtime: DateTime<Utc>,
    pub input_identity: InputIdentity,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
// CLIPPY-ALLOW: lifecycle timestamps intentionally share the domain-standard `_at` suffix.
#[allow(clippy::struct_field_names)]
pub struct TaskLifecycle {
    pub reserved_at: Option<DateTime<Utc>>,
    pub upload_started_at: Option<DateTime<Utc>>,
    pub staged_at: Option<DateTime<Utc>>,
    pub submission_started_at: Option<DateTime<Utc>>,
    pub processing_started_at: Option<DateTime<Utc>>,
    pub remote_completed_at: Option<DateTime<Utc>>,
    pub download_started_at: Option<DateTime<Utc>>,
    pub verified_at: Option<DateTime<Utc>>,
    pub publishing_started_at: Option<DateTime<Utc>>,
    pub remote_cleanup_started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PublicationEvidence {
    pub expected_output_size: Option<u64>,
    pub expected_output_sha256: Option<Sha256Digest>,
    pub destination_staging_name: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TaskRecord {
    pub id: TaskId,
    pub version: u64,
    pub status: TaskStatus,
    pub request: TaskCreateRequest,
    pub input_extension: InputExtension,
    pub output_extension: OutputExtension,
    pub input_size: u64,
    pub input_mtime: DateTime<Utc>,
    pub input_identity: Option<InputIdentity>,
    pub worker_id: Option<WorkerId>,
    pub progress: TaskProgress,
    pub attempt_count: u32,
    pub failure: Option<FailureInfo>,
    pub retry: RetryMetadata,
    pub cancel_requested_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub lifecycle: TaskLifecycle,
    pub publication: PublicationEvidence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Reservation {
    pub task_id: TaskId,
    pub expected_task_version: u64,
    pub worker_id: WorkerId,
    pub attempt_id: AttemptId,
    pub submission_key: SubmissionKey,
    pub reserved_at: DateTime<Utc>,
}

pub(crate) fn empty_progress() -> TaskProgress {
    TaskProgress {
        percent: 0.0,
        processed_frames: None,
        total_frames: None,
        frames_per_second: None,
        eta_seconds: None,
        bytes_transferred: None,
        bytes_total: None,
    }
}
