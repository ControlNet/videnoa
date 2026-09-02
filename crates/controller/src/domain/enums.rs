use serde::{Deserialize, Serialize};

macro_rules! snake_case_enum {
    ($name:ident { $($variant:ident),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $($variant),+
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
