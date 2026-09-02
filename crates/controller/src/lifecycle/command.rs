use chrono::{DateTime, Utc};

use crate::domain::{
    AttemptId, FailureInfo, RemoteJobId, RemotePath, SubmissionKey, TaskId, TaskStatus, WorkerId,
};

use super::{CancelAction, CommandKind, RemoteTerminalStatus};

pub type ReserveCommand = crate::persistence::Reservation;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubmissionEvidence {
    pub remote_job_id: RemoteJobId,
    pub remote_input_path: RemotePath,
    pub remote_output_path: RemotePath,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SubmissionCancellationReconciliation {
    Accepted(SubmissionEvidence),
    NotAccepted,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdvanceCommand {
    StartUpload,
    FinishUpload,
    StartSubmission,
    PersistSubmission(SubmissionEvidence),
    FinishProcessing,
    StartDownload,
    FinishDownload,
    FinishVerification,
    FinishPublication,
    FinishCleanup,
}

impl AdvanceCommand {
    pub(crate) const fn kind(&self) -> CommandKind {
        match self {
            Self::StartUpload => CommandKind::StartUpload,
            Self::FinishUpload => CommandKind::FinishUpload,
            Self::StartSubmission => CommandKind::StartSubmission,
            Self::PersistSubmission(_) => CommandKind::PersistSubmission,
            Self::FinishProcessing => CommandKind::FinishProcessing,
            Self::StartDownload => CommandKind::StartDownload,
            Self::FinishDownload => CommandKind::FinishDownload,
            Self::FinishVerification => CommandKind::FinishVerification,
            Self::FinishPublication => CommandKind::FinishPublication,
            Self::FinishCleanup => CommandKind::FinishCleanup,
        }
    }

    pub(crate) const fn action(&self) -> DurableAction {
        match self {
            Self::StartUpload => DurableAction::Upload,
            Self::FinishUpload | Self::FinishProcessing | Self::FinishCleanup => {
                DurableAction::None
            }
            Self::StartSubmission => DurableAction::Submit,
            Self::PersistSubmission(_) => DurableAction::Poll,
            Self::StartDownload => DurableAction::Download,
            Self::FinishDownload => DurableAction::Verify,
            Self::FinishVerification => DurableAction::Publish,
            Self::FinishPublication => DurableAction::Cleanup,
        }
    }

    pub(crate) fn submission(&self) -> Option<&SubmissionEvidence> {
        match self {
            Self::PersistSubmission(evidence) => Some(evidence),
            Self::StartUpload
            | Self::FinishUpload
            | Self::StartSubmission
            | Self::FinishProcessing
            | Self::StartDownload
            | Self::FinishDownload
            | Self::FinishVerification
            | Self::FinishPublication
            | Self::FinishCleanup => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DurableAction {
    None,
    Upload,
    Submit,
    Poll,
    Download,
    Verify,
    Publish,
    Cleanup,
    Cancel(CancelAction),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommittedCommand {
    status: TaskStatus,
    version: u64,
    action: DurableAction,
}

impl CommittedCommand {
    pub(crate) const fn new(status: TaskStatus, version: u64, action: DurableAction) -> Self {
        Self {
            status,
            version,
            action,
        }
    }

    #[must_use]
    pub const fn status(self) -> TaskStatus {
        self.status
    }

    #[must_use]
    pub const fn version(self) -> u64 {
        self.version
    }

    #[must_use]
    pub const fn action(self) -> DurableAction {
        self.action
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalRemoteEvidence {
    job_id: RemoteJobId,
    status: RemoteTerminalStatus,
}

impl TerminalRemoteEvidence {
    #[must_use]
    pub const fn new(job_id: RemoteJobId, status: RemoteTerminalStatus) -> Self {
        Self { job_id, status }
    }

    pub(crate) const fn job_id(self) -> RemoteJobId {
        self.job_id
    }

    pub(crate) const fn status(self) -> RemoteTerminalStatus {
        self.status
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceCleaned {
    task_id: TaskId,
    remote_job_id: RemoteJobId,
}

impl WorkspaceCleaned {
    #[must_use]
    pub const fn new(task_id: TaskId, remote_job_id: RemoteJobId) -> Self {
        Self {
            task_id,
            remote_job_id,
        }
    }

    pub(crate) const fn task_id(self) -> TaskId {
        self.task_id
    }

    pub(crate) const fn remote_job_id(self) -> RemoteJobId {
        self.remote_job_id
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessingRetryCommand {
    pub attempt_id: AttemptId,
    pub worker_id: WorkerId,
    pub submission_key: SubmissionKey,
    pub terminal: TerminalRemoteEvidence,
    pub workspace: WorkspaceCleaned,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AttemptCas {
    pub id: AttemptId,
    pub version: u64,
    pub status: TaskStatus,
}

#[derive(Clone, Debug)]
pub(crate) struct PairedTransition {
    pub task_id: TaskId,
    pub task_version: u64,
    pub from: TaskStatus,
    pub to: TaskStatus,
    pub attempt: AttemptCas,
    pub occurred_at: DateTime<Utc>,
    pub submission: Option<SubmissionEvidence>,
}

#[derive(Clone, Debug)]
pub(crate) struct FailureWrite {
    pub task_id: TaskId,
    pub task_version: u64,
    pub from: TaskStatus,
    pub attempt: Option<AttemptCas>,
    pub failure: FailureInfo,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CancellationWrite {
    pub task_id: TaskId,
    pub task_version: u64,
    pub from: TaskStatus,
    pub attempt: Option<AttemptCas>,
    pub requested_at: DateTime<Utc>,
    pub immediate: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RetryWrite {
    pub task_id: TaskId,
    pub task_version: u64,
    pub attempt: AttemptCas,
    pub target: TaskStatus,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ProcessingRetryWrite {
    pub task_id: TaskId,
    pub task_version: u64,
    pub old_attempt: AttemptCas,
    pub new_attempt_id: AttemptId,
    pub worker_id: WorkerId,
    pub submission_key: SubmissionKey,
    pub remote_job_id: RemoteJobId,
    pub occurred_at: DateTime<Utc>,
}
