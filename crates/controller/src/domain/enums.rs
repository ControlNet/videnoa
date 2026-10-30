use serde::{Deserialize, Serialize};

macro_rules! snake_case_enum {
    ($name:ident { $($variant:ident),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            /// Every value, in declaration order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
        }
    };
}

snake_case_enum!(TaskStatus {
    Queued,
    Reserved,
    Uploading,
    Staged,
    Submitting,
    Processing,
    RemoteCompleted,
    Downloading,
    Verifying,
    Publishing,
    RemoteCleanup,
    Completed,
    Failed,
    Cancelled,
});
impl TaskStatus {
    /// Holds one of its Worker's compute slots: the job is being submitted or runs.
    #[must_use]
    pub const fn occupies_compute(self) -> bool {
        matches!(self, Self::Submitting | Self::Processing)
    }

    /// Assigned to a Worker ahead of compute: the prefetch that waits for a slot.
    #[must_use]
    pub const fn is_stage_in(self) -> bool {
        matches!(self, Self::Reserved | Self::Uploading | Self::Staged)
    }

    /// No lifecycle command other than an explicit retry leaves this status.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

snake_case_enum!(FailureStage {
    Reservation,
    Upload,
    Submission,
    Processing,
    Download,
    Verification,
    Publication,
    LocalCleanup,
    RemoteCleanup,
});
snake_case_enum!(FailureCode {
    InputUnavailable,
    InputChanged,
    OutputExists,
    WorkerUnavailable,
    WorkflowIncompatible,
    TransferFailed,
    RemoteSubmissionFailed,
    RemoteStateAmbiguous,
    ProcessingFailed,
    VerificationFailed,
    PublicationFailed,
    PublicationAmbiguous,
    CleanupFailed,
    Cancelled,
});
snake_case_enum!(TaskSource { Manual, Api });
// Operator actions a task accepts in its current state.
snake_case_enum!(TaskAction {
    Cancel,
    Abandon,
    Retry,
    RetryOnAnotherWorker,
    Discard,
});

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskSortField {
    #[default]
    Priority,
    CreatedAt,
    CompletedAt,
    Status,
    Worker,
    Duration,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SortDirection {
    Asc,
    #[default]
    Desc,
}

snake_case_enum!(TaskFilterField {
    Status,
    Worker,
    Workflow,
    Source,
    FailureStage,
    Search,
});
snake_case_enum!(WorkflowKind { Workflow, Preset });
snake_case_enum!(SseEventKind {
    TaskUpdated,
    WorkerUpdated,
    SchedulerUpdated
});
snake_case_enum!(HealthStatus { Ok, Degraded });
snake_case_enum!(ReadinessStatus { Ready, NotReady });
snake_case_enum!(AuthMethod { Session, Bearer });
snake_case_enum!(ApiErrorCode {
    InvalidRequest,
    Unauthorized,
    Forbidden,
    NotFound,
    Conflict,
    RateLimited,
    Unavailable,
    InternalError,
    RemoteStateAmbiguous,
    PublicationAmbiguous,
});
snake_case_enum!(FieldErrorCode {
    Required,
    InvalidValue,
    UnknownValue,
    OutOfRange,
    Conflict
});
