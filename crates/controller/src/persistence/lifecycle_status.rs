use sqlx::{Sqlite, Transaction};

use crate::lifecycle::{PairedTransition, TransitionEvidence};

use super::codec::{sqlite_u64, task_status, timestamp};
use super::PersistenceError;

pub(super) async fn update_task_status(
    transaction: &mut Transaction<'_, Sqlite>,
    write: &PairedTransition,
) -> Result<u64, PersistenceError> {
    let occurred_at = timestamp(write.occurred_at);
    let status = task_status(write.to);
    let download = match write.evidence {
        TransitionEvidence::Download(evidence) => Some(evidence),
        TransitionEvidence::None
        | TransitionEvidence::Upload(_)
        | TransitionEvidence::Submission(_)
        | TransitionEvidence::Publication(_) => None,
    };
    let staging_name = match &write.evidence {
        TransitionEvidence::Publication(intent) => intent.destination_staging_name(),
        TransitionEvidence::None
        | TransitionEvidence::Upload(_)
        | TransitionEvidence::Submission(_)
        | TransitionEvidence::Download(_) => None,
    };
    let expected_size = download
        .map(|evidence| sqlite_u64("expected_output_size", evidence.size))
        .transpose()?;
    let expected_sha = download.map(|evidence| evidence.sha256.as_bytes().to_vec());
    let result = sqlx::query(
        "UPDATE tasks SET status = ?, version = version + 1, updated_at_ms = ?,
            upload_started_at_ms = CASE WHEN ? = 'uploading' THEN ? ELSE upload_started_at_ms END,
            staged_at_ms = CASE WHEN ? = 'staged' THEN ? ELSE staged_at_ms END,
            submission_started_at_ms = CASE WHEN ? = 'submitting' THEN ? ELSE submission_started_at_ms END,
            processing_started_at_ms = CASE WHEN ? = 'processing' THEN ? ELSE processing_started_at_ms END,
            remote_completed_at_ms = CASE WHEN ? = 'remote_completed' THEN ? ELSE remote_completed_at_ms END,
            download_started_at_ms = CASE WHEN ? = 'downloading' THEN ? ELSE download_started_at_ms END,
            verified_at_ms = CASE WHEN ? = 'verifying' THEN ? ELSE verified_at_ms END,
            publishing_started_at_ms = CASE WHEN ? = 'publishing' THEN ? ELSE publishing_started_at_ms END,
            remote_cleanup_started_at_ms = CASE WHEN ? = 'remote_cleanup' THEN ? ELSE remote_cleanup_started_at_ms END,
            completed_at_ms = CASE WHEN ? = 'completed' THEN ? ELSE completed_at_ms END,
            expected_output_size = CASE WHEN ? = 'verifying' THEN ? ELSE expected_output_size END,
            expected_output_sha256 = CASE WHEN ? = 'verifying' THEN ? ELSE expected_output_sha256 END,
            destination_staging_name = CASE WHEN ? = 'publishing' THEN ? ELSE destination_staging_name END,
            retry_count = CASE WHEN ? IN ('staged', 'verifying') THEN 0 ELSE retry_count END,
            next_retry_at_ms = CASE WHEN ? IN ('staged', 'verifying') THEN NULL ELSE next_retry_at_ms END
         WHERE (? != 'uploading' OR (SELECT paused FROM controller_settings WHERE id = 1) = 0)
           AND (? != 'submitting' OR (
               (SELECT paused FROM controller_settings WHERE id = 1) = 0
               AND (SELECT COUNT(*) FROM tasks active
                    WHERE active.worker_id = tasks.worker_id
                      AND active.status IN ('submitting', 'processing')) <
                   (SELECT compute_slots FROM workers worker WHERE worker.id = tasks.worker_id)
           ))
           AND id = ? AND status = ? AND version = ?",
    )
    .bind(status).bind(occurred_at)
    .bind(status).bind(occurred_at).bind(status).bind(occurred_at)
    .bind(status).bind(occurred_at).bind(status).bind(occurred_at)
    .bind(status).bind(occurred_at).bind(status).bind(occurred_at)
    .bind(status).bind(occurred_at).bind(status).bind(occurred_at)
    .bind(status).bind(occurred_at).bind(status).bind(occurred_at)
    .bind(status).bind(expected_size).bind(status).bind(expected_sha)
    .bind(status).bind(staging_name).bind(status).bind(status).bind(status).bind(status)
    .bind(write.task_id.to_string()).bind(task_status(write.from))
    .bind(sqlite_u64("task_version", write.task_version)?)
    .execute(&mut **transaction).await?;
    Ok(result.rows_affected())
}

pub(super) async fn update_attempt_status(
    transaction: &mut Transaction<'_, Sqlite>,
    write: &PairedTransition,
) -> Result<u64, PersistenceError> {
    let occurred_at = timestamp(write.occurred_at);
    let status = task_status(write.to);
    let result = sqlx::query(
        "UPDATE task_attempts SET status = ?, version = version + 1, updated_at_ms = ?,
            started_at_ms = CASE WHEN ? = 'processing' THEN ? ELSE started_at_ms END,
            completed_at_ms = CASE WHEN ? IN ('completed', 'failed', 'cancelled')
                THEN ? ELSE completed_at_ms END,
            retry_count = CASE WHEN ? IN ('staged', 'verifying') THEN 0 ELSE retry_count END,
            next_retry_at_ms = CASE WHEN ? IN ('staged', 'verifying') THEN NULL ELSE next_retry_at_ms END
         WHERE id = ? AND status = ? AND version = ?",
    )
    .bind(status).bind(occurred_at).bind(status).bind(occurred_at)
    .bind(status).bind(occurred_at).bind(status).bind(status)
    .bind(write.attempt.id.to_string()).bind(task_status(write.attempt.status))
    .bind(sqlite_u64("attempt_version", write.attempt.version)?)
    .execute(&mut **transaction).await?;
    Ok(result.rows_affected())
}
