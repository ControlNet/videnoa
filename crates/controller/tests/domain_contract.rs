use std::error::Error;

use serde::de::DeserializeOwned;
use serde::Serialize;
use videnoa_controller::domain::{
    ApiErrorCode, AuthMethod, FailureCode, FailureStage, FieldErrorCode, HealthStatus,
    ReadinessStatus, SortDirection, SseEventKind, TaskFilterField, TaskSortField, TaskSource,
    TaskStatus, WorkflowKind,
};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

fn assert_enum_contract<T>(cases: &[(T, &str)]) -> TestResult
where
    T: Clone + DeserializeOwned + PartialEq + Serialize + std::fmt::Debug,
{
    for (value, spelling) in cases {
        assert_eq!(serde_json::to_string(value)?, format!("\"{spelling}\""));
        assert_eq!(
            serde_json::from_str::<T>(&format!("\"{spelling}\""))?,
            *value
        );
    }
    assert!(serde_json::from_str::<T>("\"unknown_contract_value\"").is_err());
    Ok(())
}

#[test]
fn lifecycle_and_failure_enums_use_stable_snake_case() -> TestResult {
    // Given: every lifecycle, failure, and task-source enum variant.
    // When/Then: each value round-trips through its locked spelling and rejects unknown values.
    assert_enum_contract(&[
        (TaskStatus::Queued, "queued"),
        (TaskStatus::Reserved, "reserved"),
        (TaskStatus::Uploading, "uploading"),
        (TaskStatus::Staged, "staged"),
        (TaskStatus::Submitting, "submitting"),
        (TaskStatus::Processing, "processing"),
        (TaskStatus::RemoteCompleted, "remote_completed"),
        (TaskStatus::Downloading, "downloading"),
        (TaskStatus::Verifying, "verifying"),
        (TaskStatus::Publishing, "publishing"),
        (TaskStatus::RemoteCleanup, "remote_cleanup"),
        (TaskStatus::Completed, "completed"),
        (TaskStatus::Failed, "failed"),
        (TaskStatus::Cancelled, "cancelled"),
    ])?;
    assert_enum_contract(&[
        (FailureStage::Reservation, "reservation"),
        (FailureStage::Upload, "upload"),
        (FailureStage::Submission, "submission"),
        (FailureStage::Processing, "processing"),
        (FailureStage::Download, "download"),
        (FailureStage::Verification, "verification"),
        (FailureStage::Publication, "publication"),
        (FailureStage::LocalCleanup, "local_cleanup"),
        (FailureStage::RemoteCleanup, "remote_cleanup"),
    ])?;
    assert_enum_contract(&[
        (FailureCode::InputUnavailable, "input_unavailable"),
        (FailureCode::InputChanged, "input_changed"),
        (FailureCode::OutputExists, "output_exists"),
        (FailureCode::WorkerUnavailable, "worker_unavailable"),
        (FailureCode::WorkflowIncompatible, "workflow_incompatible"),
        (FailureCode::TransferFailed, "transfer_failed"),
        (
            FailureCode::RemoteSubmissionFailed,
            "remote_submission_failed",
        ),
        (FailureCode::RemoteStateAmbiguous, "remote_state_ambiguous"),
        (FailureCode::ProcessingFailed, "processing_failed"),
        (FailureCode::VerificationFailed, "verification_failed"),
        (FailureCode::PublicationFailed, "publication_failed"),
        (FailureCode::PublicationAmbiguous, "publication_ambiguous"),
        (FailureCode::CleanupFailed, "cleanup_failed"),
        (FailureCode::Cancelled, "cancelled"),
    ])?;
    assert_enum_contract(&[(TaskSource::Manual, "manual"), (TaskSource::Api, "api")])
}

#[test]
fn query_and_http_enums_use_stable_snake_case() -> TestResult {
    // Given: every query, auth, system, SSE, and API-error enum variant.
    // When/Then: each value round-trips through its locked spelling and rejects unknown values.
    assert_enum_contract(&[
        (TaskSortField::Priority, "priority"),
        (TaskSortField::CreatedAt, "created_at"),
        (TaskSortField::CompletedAt, "completed_at"),
        (TaskSortField::Status, "status"),
        (TaskSortField::Worker, "worker"),
        (TaskSortField::Duration, "duration"),
    ])?;
    assert_enum_contract(&[(SortDirection::Asc, "asc"), (SortDirection::Desc, "desc")])?;
    assert_enum_contract(&[
        (TaskFilterField::Status, "status"),
        (TaskFilterField::Worker, "worker"),
        (TaskFilterField::Workflow, "workflow"),
        (TaskFilterField::Source, "source"),
        (TaskFilterField::FailureStage, "failure_stage"),
        (TaskFilterField::Search, "search"),
    ])?;
    assert_enum_contract(&[
        (WorkflowKind::Workflow, "workflow"),
        (WorkflowKind::Preset, "preset"),
    ])?;
    assert_enum_contract(&[
        (SseEventKind::TaskUpdated, "task_updated"),
        (SseEventKind::WorkerUpdated, "worker_updated"),
        (SseEventKind::SchedulerUpdated, "scheduler_updated"),
    ])?;
    assert_enum_contract(&[
        (HealthStatus::Ok, "ok"),
        (HealthStatus::Degraded, "degraded"),
    ])?;
    assert_enum_contract(&[
        (ReadinessStatus::Ready, "ready"),
        (ReadinessStatus::NotReady, "not_ready"),
    ])?;
    assert_enum_contract(&[
        (AuthMethod::Session, "session"),
        (AuthMethod::Bearer, "bearer"),
    ])?;
    assert_enum_contract(&[
        (ApiErrorCode::InvalidRequest, "invalid_request"),
        (ApiErrorCode::Unauthorized, "unauthorized"),
        (ApiErrorCode::Forbidden, "forbidden"),
        (ApiErrorCode::NotFound, "not_found"),
        (ApiErrorCode::Conflict, "conflict"),
        (ApiErrorCode::RateLimited, "rate_limited"),
        (ApiErrorCode::Unavailable, "unavailable"),
        (ApiErrorCode::InternalError, "internal_error"),
        (ApiErrorCode::RemoteStateAmbiguous, "remote_state_ambiguous"),
        (ApiErrorCode::PublicationAmbiguous, "publication_ambiguous"),
    ])?;
    assert_enum_contract(&[
        (FieldErrorCode::Required, "required"),
        (FieldErrorCode::InvalidValue, "invalid_value"),
        (FieldErrorCode::UnknownValue, "unknown_value"),
        (FieldErrorCode::OutOfRange, "out_of_range"),
        (FieldErrorCode::Conflict, "conflict"),
    ])
}
