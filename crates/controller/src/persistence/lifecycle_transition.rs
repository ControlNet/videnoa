use sqlx::{Sqlite, Transaction};

use crate::lifecycle::{CancellationWrite, FailureWrite, PairedTransition, SubmissionEvidence};

use super::codec::{failure_code, failure_stage, sqlite_u64, task_status, timestamp};
use super::{CasOutcome, PersistenceError, Store};

impl Store {
    pub(crate) async fn apply_lifecycle_transition(
        &self,
        write: &PairedTransition,
    ) -> Result<CasOutcome, PersistenceError> {
        let mut transaction = self.database.pool().begin().await?;
        if update_task_status(&mut transaction, write).await? != 1 {
            transaction.rollback().await?;
            return Ok(CasOutcome::Conflict);
        }
        let attempt_rows = match write.submission.as_ref() {
            Some(evidence) => bind_submission(&mut transaction, write, evidence).await?,
            None => update_attempt_status(&mut transaction, write).await?,
        };
        if attempt_rows != 1 {
            transaction.rollback().await?;
            return Ok(CasOutcome::Conflict);
        }
        transaction.commit().await?;
        Ok(CasOutcome::Applied {
            new_version: write.task_version + 1,
        })
    }

    pub(crate) async fn fail_lifecycle(
        &self,
        write: &FailureWrite,
    ) -> Result<CasOutcome, PersistenceError> {
        let mut transaction = self.database.pool().begin().await?;
        let occurred_at = timestamp(write.occurred_at);
        let result = sqlx::query(
            "UPDATE tasks SET status = 'failed', failure_stage = ?, failure_code = ?,
                failure_message = ?, failure_retryable = ?, next_retry_at_ms = NULL,
                version = version + 1, updated_at_ms = ?
             WHERE id = ? AND status = ? AND version = ?",
        )
        .bind(failure_stage(write.failure.failure_stage))
        .bind(failure_code(write.failure.failure_code))
        .bind(write.failure.message.as_str())
        .bind(write.failure.retryable)
        .bind(occurred_at)
        .bind(write.task_id.to_string())
        .bind(task_status(write.from))
        .bind(sqlite_u64("task_version", write.task_version)?)
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() != 1 {
            transaction.rollback().await?;
            return Ok(CasOutcome::Conflict);
        }
        if let Some(attempt) = write.attempt {
            let result = sqlx::query(
                "UPDATE task_attempts SET status = 'failed', failure_stage = ?, failure_code = ?,
                    failure_message = ?, failure_retryable = ?, next_retry_at_ms = NULL,
                    completed_at_ms = ?, version = version + 1, updated_at_ms = ?
                 WHERE id = ? AND status = ? AND version = ?",
            )
            .bind(failure_stage(write.failure.failure_stage))
            .bind(failure_code(write.failure.failure_code))
            .bind(write.failure.message.as_str())
            .bind(write.failure.retryable)
            .bind(occurred_at)
            .bind(occurred_at)
            .bind(attempt.id.to_string())
            .bind(task_status(attempt.status))
            .bind(sqlite_u64("attempt_version", attempt.version)?)
            .execute(&mut *transaction)
            .await?;
            if result.rows_affected() != 1 {
                transaction.rollback().await?;
                return Ok(CasOutcome::Conflict);
            }
        }
        transaction.commit().await?;
        Ok(CasOutcome::Applied {
            new_version: write.task_version + 1,
        })
    }

    pub(crate) async fn request_lifecycle_cancellation(
        &self,
        write: &CancellationWrite,
    ) -> Result<CasOutcome, PersistenceError> {
        let mut transaction = self.database.pool().begin().await?;
        let requested_at = timestamp(write.requested_at);
        let next_status = if write.immediate {
            "cancelled"
        } else {
            task_status(write.from)
        };
        let result = sqlx::query(
            "UPDATE tasks SET status = ?, cancel_requested_at_ms = ?, version = version + 1,
                updated_at_ms = ? WHERE id = ? AND status = ? AND version = ?
                AND cancel_requested_at_ms IS NULL",
        )
        .bind(next_status)
        .bind(requested_at)
        .bind(requested_at)
        .bind(write.task_id.to_string())
        .bind(task_status(write.from))
        .bind(sqlite_u64("task_version", write.task_version)?)
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() != 1 {
            transaction.rollback().await?;
            return Ok(CasOutcome::Conflict);
        }
        if write.immediate {
            if let Some(attempt) = write.attempt {
                let result = sqlx::query(
                    "UPDATE task_attempts SET status = 'cancelled', completed_at_ms = ?,
                        version = version + 1, updated_at_ms = ?
                     WHERE id = ? AND status = ? AND version = ?",
                )
                .bind(requested_at)
                .bind(requested_at)
                .bind(attempt.id.to_string())
                .bind(task_status(attempt.status))
                .bind(sqlite_u64("attempt_version", attempt.version)?)
                .execute(&mut *transaction)
                .await?;
                if result.rows_affected() != 1 {
                    transaction.rollback().await?;
                    return Ok(CasOutcome::Conflict);
                }
            }
        }
        transaction.commit().await?;
        Ok(CasOutcome::Applied {
            new_version: write.task_version + 1,
        })
    }
}

async fn update_task_status(
    transaction: &mut Transaction<'_, Sqlite>,
    write: &PairedTransition,
) -> Result<u64, PersistenceError> {
    let occurred_at = timestamp(write.occurred_at);
    let status = task_status(write.to);
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
            completed_at_ms = CASE WHEN ? = 'completed' THEN ? ELSE completed_at_ms END
         WHERE id = ? AND status = ? AND version = ?",
    )
    .bind(status)
    .bind(occurred_at)
    .bind(status).bind(occurred_at)
    .bind(status).bind(occurred_at)
    .bind(status).bind(occurred_at)
    .bind(status).bind(occurred_at)
    .bind(status).bind(occurred_at)
    .bind(status).bind(occurred_at)
    .bind(status).bind(occurred_at)
    .bind(status).bind(occurred_at)
    .bind(status).bind(occurred_at)
    .bind(status).bind(occurred_at)
    .bind(write.task_id.to_string())
    .bind(task_status(write.from))
    .bind(sqlite_u64("task_version", write.task_version)?)
    .execute(&mut **transaction)
    .await?;
    Ok(result.rows_affected())
}

async fn update_attempt_status(
    transaction: &mut Transaction<'_, Sqlite>,
    write: &PairedTransition,
) -> Result<u64, PersistenceError> {
    let occurred_at = timestamp(write.occurred_at);
    let status = task_status(write.to);
    let result = sqlx::query(
        "UPDATE task_attempts SET status = ?, version = version + 1, updated_at_ms = ?,
            started_at_ms = CASE WHEN ? = 'processing' THEN ? ELSE started_at_ms END,
            completed_at_ms = CASE WHEN ? IN ('completed', 'failed', 'cancelled')
                THEN ? ELSE completed_at_ms END
         WHERE id = ? AND status = ? AND version = ?",
    )
    .bind(status)
    .bind(occurred_at)
    .bind(status)
    .bind(occurred_at)
    .bind(status)
    .bind(occurred_at)
    .bind(write.attempt.id.to_string())
    .bind(task_status(write.attempt.status))
    .bind(sqlite_u64("attempt_version", write.attempt.version)?)
    .execute(&mut **transaction)
    .await?;
    Ok(result.rows_affected())
}

async fn bind_submission(
    transaction: &mut Transaction<'_, Sqlite>,
    write: &PairedTransition,
    evidence: &SubmissionEvidence,
) -> Result<u64, PersistenceError> {
    let occurred_at = timestamp(write.occurred_at);
    let result = sqlx::query(
        "UPDATE task_attempts SET status = 'processing', remote_job_id = ?,
            remote_input_path = ?, remote_output_path = ?, submitted_at_ms = ?,
            started_at_ms = ?, version = version + 1, updated_at_ms = ?
         WHERE id = ? AND status = 'submitting' AND version = ?
           AND remote_job_id IS NULL AND remote_input_path IS NULL AND remote_output_path IS NULL",
    )
    .bind(evidence.remote_job_id.to_string())
    .bind(evidence.remote_input_path.as_str())
    .bind(evidence.remote_output_path.as_str())
    .bind(occurred_at)
    .bind(occurred_at)
    .bind(occurred_at)
    .bind(write.attempt.id.to_string())
    .bind(sqlite_u64("attempt_version", write.attempt.version)?)
    .execute(&mut **transaction)
    .await?;
    Ok(result.rows_affected())
}
