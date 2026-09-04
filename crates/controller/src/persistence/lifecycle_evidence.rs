use sqlx::{Sqlite, Transaction};

use crate::lifecycle::{PairedTransition, SubmissionEvidence, UploadEvidence};

use super::codec::{sqlite_u64, timestamp};
use super::PersistenceError;

pub(super) async fn bind_upload(
    transaction: &mut Transaction<'_, Sqlite>,
    write: &PairedTransition,
    evidence: &UploadEvidence,
) -> Result<u64, PersistenceError> {
    let result = sqlx::query(
        "UPDATE task_attempts SET status = 'staged', remote_input_path = ?,
            remote_output_path = ?, retry_count = 0, next_retry_at_ms = NULL,
            version = version + 1, updated_at_ms = ?
         WHERE id = ? AND status = 'uploading' AND version = ?
           AND remote_input_path IS NULL AND remote_output_path IS NULL",
    )
    .bind(evidence.remote_input_path.as_str())
    .bind(evidence.remote_output_path.as_str())
    .bind(timestamp(write.occurred_at))
    .bind(write.attempt.id.to_string())
    .bind(sqlite_u64("attempt_version", write.attempt.version)?)
    .execute(&mut **transaction)
    .await?;
    Ok(result.rows_affected())
}

pub(super) async fn bind_submission(
    transaction: &mut Transaction<'_, Sqlite>,
    write: &PairedTransition,
    evidence: &SubmissionEvidence,
) -> Result<u64, PersistenceError> {
    let occurred_at = timestamp(write.occurred_at);
    let result = sqlx::query(
        "UPDATE task_attempts SET status = 'processing', remote_job_id = ?, submission_owner = NULL,
            submitted_at_ms = ?, started_at_ms = ?, version = version + 1, updated_at_ms = ?
         WHERE id = ? AND status = 'submitting' AND version = ?
           AND remote_job_id IS NULL AND remote_input_path = ? AND remote_output_path = ?",
    )
    .bind(evidence.remote_job_id.to_string())
    .bind(occurred_at)
    .bind(occurred_at)
    .bind(occurred_at)
    .bind(write.attempt.id.to_string())
    .bind(sqlite_u64("attempt_version", write.attempt.version)?)
    .bind(evidence.remote_input_path.as_str())
    .bind(evidence.remote_output_path.as_str())
    .execute(&mut **transaction)
    .await?;
    Ok(result.rows_affected())
}
