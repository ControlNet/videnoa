use chrono::{DateTime, Utc};

use crate::domain::{RemotePath, RetryMetadata, TaskId};
use crate::persistence::Sha256Digest;

use super::{AttemptCas, SubmissionEvidence};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UploadEvidence {
    pub remote_input_path: RemotePath,
    pub remote_output_path: RemotePath,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DownloadEvidence {
    pub size: u64,
    pub sha256: Sha256Digest,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TransitionEvidence {
    None,
    Upload(UploadEvidence),
    Submission(SubmissionEvidence),
    Download(DownloadEvidence),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TransferRetryWrite {
    pub task_id: TaskId,
    pub task_version: u64,
    pub attempt: AttemptCas,
    pub retry: RetryMetadata,
    pub occurred_at: DateTime<Utc>,
}
