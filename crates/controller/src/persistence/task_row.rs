use sqlx::sqlite::SqliteRow;
use sqlx::Row;

use crate::domain::{
    FailureInfo, InputExtension, InputPath, OutputExtension, OutputPath, RetryMetadata,
    SourceReference, TaskCreateRequest, TaskId, TaskProgress, WorkerId, WorkflowName,
};

use super::codec::{
    boolean, corrupt, decode_json, parse_brand, parse_failure_code, parse_failure_stage,
    parse_optional_timestamp, parse_task_source, parse_task_status, parse_timestamp, rust_u32,
    rust_u64,
};
use super::models::{PublicationEvidence, Sha256Digest, TaskLifecycle, TaskRecord};
use super::PersistenceError;

pub(super) const TASK_COLUMNS: &str = "id, version, status, input_path, output_path, \
    input_extension, output_extension, workflow, priority, source, source_reference, input_size, \
    input_mtime_ms, worker_id, progress_json, attempt_count, failure_stage, failure_code, \
    failure_message, failure_retryable, retry_count, next_retry_at_ms, cancel_requested_at_ms, \
    created_at_ms, updated_at_ms, reserved_at_ms, upload_started_at_ms, staged_at_ms, \
    submission_started_at_ms, processing_started_at_ms, remote_completed_at_ms, \
    download_started_at_ms, verified_at_ms, publishing_started_at_ms, \
    remote_cleanup_started_at_ms, completed_at_ms, expected_output_size, \
    expected_output_sha256, destination_staging_name";

pub(super) fn map_task(row: &SqliteRow) -> Result<TaskRecord, PersistenceError> {
    let failure_stage: Option<String> = row.try_get("failure_stage")?;
    let failure_code: Option<String> = row.try_get("failure_code")?;
    let failure_message: Option<String> = row.try_get("failure_message")?;
    let failure_retryable: Option<i64> = row.try_get("failure_retryable")?;
    let failure = match (
        failure_stage,
        failure_code,
        failure_message,
        failure_retryable,
    ) {
        (None, None, None, None) => None,
        (Some(stage), Some(code), Some(message), Some(retryable)) => Some(FailureInfo {
            failure_stage: parse_failure_stage(&stage)?,
            failure_code: parse_failure_code(&code)?,
            message,
            retryable: boolean("failure_retryable", retryable)?,
        }),
        values => return Err(corrupt("failure", format_args!("{values:?}"))),
    };
    let sha: Option<Vec<u8>> = row.try_get("expected_output_sha256")?;
    let expected_output_sha256 = sha
        .map(|bytes| {
            <[u8; 32]>::try_from(bytes)
                .map(Sha256Digest::new)
                .map_err(|bytes| corrupt("expected_output_sha256", bytes.len()))
        })
        .transpose()?;
    let worker_id = row
        .try_get::<Option<String>, _>("worker_id")?
        .map(|value| parse_brand::<WorkerId>("worker_id", &value))
        .transpose()?;
    let source_reference = row
        .try_get::<Option<String>, _>("source_reference")?
        .map(SourceReference::new);
    Ok(TaskRecord {
        id: parse_brand::<TaskId>("id", row.try_get("id")?)?,
        version: rust_u64("version", row.try_get("version")?)?,
        status: parse_task_status(row.try_get("status")?)?,
        request: TaskCreateRequest {
            input_path: InputPath::new(row.try_get::<String, _>("input_path")?),
            output_path: OutputPath::new(row.try_get::<String, _>("output_path")?),
            workflow: WorkflowName::new(row.try_get::<String, _>("workflow")?),
            priority: row.try_get("priority")?,
            source: parse_task_source(row.try_get("source")?)?,
            source_reference,
        },
        input_extension: InputExtension::new(row.try_get::<String, _>("input_extension")?),
        output_extension: OutputExtension::new(row.try_get::<String, _>("output_extension")?),
        input_size: rust_u64("input_size", row.try_get("input_size")?)?,
        input_mtime: parse_timestamp("input_mtime_ms", row.try_get("input_mtime_ms")?)?,
        worker_id,
        progress: decode_json::<TaskProgress>("progress_json", row.try_get("progress_json")?)?,
        attempt_count: rust_u32("attempt_count", row.try_get("attempt_count")?)?,
        failure,
        retry: RetryMetadata {
            retry_count: rust_u32("retry_count", row.try_get("retry_count")?)?,
            next_retry_at: parse_optional_timestamp(
                "next_retry_at_ms",
                row.try_get("next_retry_at_ms")?,
            )?,
        },
        cancel_requested_at: parse_optional_timestamp(
            "cancel_requested_at_ms",
            row.try_get("cancel_requested_at_ms")?,
        )?,
        created_at: parse_timestamp("created_at_ms", row.try_get("created_at_ms")?)?,
        updated_at: parse_timestamp("updated_at_ms", row.try_get("updated_at_ms")?)?,
        lifecycle: lifecycle(row)?,
        publication: PublicationEvidence {
            expected_output_size: row
                .try_get::<Option<i64>, _>("expected_output_size")?
                .map(|value| rust_u64("expected_output_size", value))
                .transpose()?,
            expected_output_sha256,
            destination_staging_name: row.try_get("destination_staging_name")?,
        },
    })
}

fn lifecycle(row: &SqliteRow) -> Result<TaskLifecycle, PersistenceError> {
    Ok(TaskLifecycle {
        reserved_at: optional_time(row, "reserved_at_ms")?,
        upload_started_at: optional_time(row, "upload_started_at_ms")?,
        staged_at: optional_time(row, "staged_at_ms")?,
        submission_started_at: optional_time(row, "submission_started_at_ms")?,
        processing_started_at: optional_time(row, "processing_started_at_ms")?,
        remote_completed_at: optional_time(row, "remote_completed_at_ms")?,
        download_started_at: optional_time(row, "download_started_at_ms")?,
        verified_at: optional_time(row, "verified_at_ms")?,
        publishing_started_at: optional_time(row, "publishing_started_at_ms")?,
        remote_cleanup_started_at: optional_time(row, "remote_cleanup_started_at_ms")?,
        completed_at: optional_time(row, "completed_at_ms")?,
    })
}

fn optional_time(
    row: &SqliteRow,
    field: &'static str,
) -> Result<Option<chrono::DateTime<chrono::Utc>>, PersistenceError> {
    parse_optional_timestamp(field, row.try_get(field)?)
}
