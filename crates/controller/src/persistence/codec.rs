use std::fmt::Display;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::domain::{FailureCode, FailureStage, TaskSource, TaskStatus};

use super::PersistenceError;

pub(super) const fn task_status(value: TaskStatus) -> &'static str {
    match value {
        TaskStatus::Queued => "queued",
        TaskStatus::Reserved => "reserved",
        TaskStatus::Uploading => "uploading",
        TaskStatus::Staged => "staged",
        TaskStatus::Submitting => "submitting",
        TaskStatus::Processing => "processing",
        TaskStatus::RemoteCompleted => "remote_completed",
        TaskStatus::Downloading => "downloading",
        TaskStatus::Verifying => "verifying",
        TaskStatus::Publishing => "publishing",
        TaskStatus::RemoteCleanup => "remote_cleanup",
        TaskStatus::Completed => "completed",
        TaskStatus::Failed => "failed",
        TaskStatus::Cancelled => "cancelled",
    }
}

pub(super) fn parse_task_status(value: &str) -> Result<TaskStatus, PersistenceError> {
    match value {
        "queued" => Ok(TaskStatus::Queued),
        "reserved" => Ok(TaskStatus::Reserved),
        "uploading" => Ok(TaskStatus::Uploading),
        "staged" => Ok(TaskStatus::Staged),
        "submitting" => Ok(TaskStatus::Submitting),
        "processing" => Ok(TaskStatus::Processing),
        "remote_completed" => Ok(TaskStatus::RemoteCompleted),
        "downloading" => Ok(TaskStatus::Downloading),
        "verifying" => Ok(TaskStatus::Verifying),
        "publishing" => Ok(TaskStatus::Publishing),
        "remote_cleanup" => Ok(TaskStatus::RemoteCleanup),
        "completed" => Ok(TaskStatus::Completed),
        "failed" => Ok(TaskStatus::Failed),
        "cancelled" => Ok(TaskStatus::Cancelled),
        unknown => Err(corrupt("status", unknown)),
    }
}

pub(super) const fn task_source(value: TaskSource) -> &'static str {
    match value {
        TaskSource::Manual => "manual",
        TaskSource::Api => "api",
    }
}

pub(super) fn parse_task_source(value: &str) -> Result<TaskSource, PersistenceError> {
    match value {
        "manual" => Ok(TaskSource::Manual),
        "api" => Ok(TaskSource::Api),
        unknown => Err(corrupt("source", unknown)),
    }
}

pub(super) const fn failure_stage(value: FailureStage) -> &'static str {
    match value {
        FailureStage::Reservation => "reservation",
        FailureStage::Upload => "upload",
        FailureStage::Submission => "submission",
        FailureStage::Processing => "processing",
        FailureStage::Download => "download",
        FailureStage::Verification => "verification",
        FailureStage::Publication => "publication",
        FailureStage::LocalCleanup => "local_cleanup",
        FailureStage::RemoteCleanup => "remote_cleanup",
    }
}

pub(super) fn parse_failure_stage(value: &str) -> Result<FailureStage, PersistenceError> {
    match value {
        "reservation" => Ok(FailureStage::Reservation),
        "upload" => Ok(FailureStage::Upload),
        "submission" => Ok(FailureStage::Submission),
        "processing" => Ok(FailureStage::Processing),
        "download" => Ok(FailureStage::Download),
        "verification" => Ok(FailureStage::Verification),
        "publication" => Ok(FailureStage::Publication),
        "local_cleanup" => Ok(FailureStage::LocalCleanup),
        "remote_cleanup" => Ok(FailureStage::RemoteCleanup),
        unknown => Err(corrupt("failure_stage", unknown)),
    }
}

pub(super) const fn failure_code(value: FailureCode) -> &'static str {
    match value {
        FailureCode::InputUnavailable => "input_unavailable",
        FailureCode::InputChanged => "input_changed",
        FailureCode::OutputExists => "output_exists",
        FailureCode::WorkerUnavailable => "worker_unavailable",
        FailureCode::WorkflowIncompatible => "workflow_incompatible",
        FailureCode::TransferFailed => "transfer_failed",
        FailureCode::RemoteSubmissionFailed => "remote_submission_failed",
        FailureCode::RemoteStateAmbiguous => "remote_state_ambiguous",
        FailureCode::ProcessingFailed => "processing_failed",
        FailureCode::VerificationFailed => "verification_failed",
        FailureCode::PublicationFailed => "publication_failed",
        FailureCode::PublicationAmbiguous => "publication_ambiguous",
        FailureCode::CleanupFailed => "cleanup_failed",
        FailureCode::Cancelled => "cancelled",
    }
}

pub(super) fn parse_failure_code(value: &str) -> Result<FailureCode, PersistenceError> {
    match value {
        "input_unavailable" => Ok(FailureCode::InputUnavailable),
        "input_changed" => Ok(FailureCode::InputChanged),
        "output_exists" => Ok(FailureCode::OutputExists),
        "worker_unavailable" => Ok(FailureCode::WorkerUnavailable),
        "workflow_incompatible" => Ok(FailureCode::WorkflowIncompatible),
        "transfer_failed" => Ok(FailureCode::TransferFailed),
        "remote_submission_failed" => Ok(FailureCode::RemoteSubmissionFailed),
        "remote_state_ambiguous" => Ok(FailureCode::RemoteStateAmbiguous),
        "processing_failed" => Ok(FailureCode::ProcessingFailed),
        "verification_failed" => Ok(FailureCode::VerificationFailed),
        "publication_failed" => Ok(FailureCode::PublicationFailed),
        "publication_ambiguous" => Ok(FailureCode::PublicationAmbiguous),
        "cleanup_failed" => Ok(FailureCode::CleanupFailed),
        "cancelled" => Ok(FailureCode::Cancelled),
        unknown => Err(corrupt("failure_code", unknown)),
    }
}

pub(super) fn timestamp(value: DateTime<Utc>) -> i64 {
    value.timestamp_millis()
}

pub(super) fn parse_timestamp(
    field: &'static str,
    value: i64,
) -> Result<DateTime<Utc>, PersistenceError> {
    DateTime::from_timestamp_millis(value).ok_or_else(|| corrupt(field, value))
}

pub(super) fn parse_optional_timestamp(
    field: &'static str,
    value: Option<i64>,
) -> Result<Option<DateTime<Utc>>, PersistenceError> {
    value.map(|raw| parse_timestamp(field, raw)).transpose()
}

pub(super) fn sqlite_u64(field: &'static str, value: u64) -> Result<i64, PersistenceError> {
    i64::try_from(value).map_err(|_| PersistenceError::NumericOverflow { field })
}

pub(super) fn rust_u64(field: &'static str, value: i64) -> Result<u64, PersistenceError> {
    u64::try_from(value).map_err(|_| corrupt(field, value))
}

pub(super) fn rust_u32(field: &'static str, value: i64) -> Result<u32, PersistenceError> {
    u32::try_from(value).map_err(|_| corrupt(field, value))
}

pub(super) fn boolean(field: &'static str, value: i64) -> Result<bool, PersistenceError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        unknown => Err(corrupt(field, unknown)),
    }
}

pub(super) fn encode_json<T: Serialize>(
    field: &'static str,
    value: &T,
) -> Result<String, PersistenceError> {
    serde_json::to_string(value).map_err(|source| PersistenceError::Json { field, source })
}

pub(super) fn decode_json<T: DeserializeOwned>(
    field: &'static str,
    value: &str,
) -> Result<T, PersistenceError> {
    serde_json::from_str(value).map_err(|source| PersistenceError::Json { field, source })
}

pub(super) fn parse_brand<T>(field: &'static str, value: &str) -> Result<T, PersistenceError>
where
    T: FromStr,
    T::Err: Display,
{
    value.parse().map_err(|_| corrupt(field, value))
}

pub(super) fn corrupt(field: &'static str, value: impl Display) -> PersistenceError {
    PersistenceError::CorruptValue {
        field,
        value: value.to_string(),
    }
}
