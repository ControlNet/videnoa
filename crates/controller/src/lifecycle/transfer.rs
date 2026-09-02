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
pub struct PublicationIntent {
    destination_staging_name: String,
}

impl PublicationIntent {
    #[must_use]
    pub fn new(destination_staging_name: impl Into<String>) -> Self {
        Self {
            destination_staging_name: destination_staging_name.into(),
        }
    }

    pub(crate) fn destination_staging_name(&self) -> &str {
        &self.destination_staging_name
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TransitionEvidence {
    None,
    Upload(UploadEvidence),
    Submission(SubmissionEvidence),
    Download(DownloadEvidence),
    Publication(PublicationIntent),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TransferRetryWrite {
    pub task_id: TaskId,
    pub task_version: u64,
    pub attempt: AttemptCas,
    pub retry: RetryMetadata,
    pub occurred_at: DateTime<Utc>,
}
