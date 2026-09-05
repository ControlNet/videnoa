use sqlx::Row;

use crate::lifecycle::{ProcessingRetryWrite, RetryWrite};

use super::codec::{encode_json, sqlite_u64, task_status, timestamp};
use super::models::empty_progress;
use super::{CasOutcome, PersistenceError, Store};

impl Store {
    pub(crate) async fn retry_lifecycle_stage(
        &self,
        write: &RetryWrite,
    ) -> Result<CasOutcome, PersistenceError> {
        let mut transaction = self.database.pool().begin().await?;
        let occurred_at = timestamp(write.occurred_at);
        let status = task_status(write.target);
        let result = sqlx::query(
            "UPDATE tasks SET status = ?, failure_stage = NULL, failure_code = NULL,
                failure_message = NULL, failure_retryable = NULL, retry_count = 0,
                next_retry_at_ms = NULL, version = version + 1, updated_at_ms = ?,
                upload_started_at_ms = CASE WHEN ? = 'uploading' THEN ? ELSE upload_started_at_ms END,
                download_started_at_ms = CASE WHEN ? = 'downloading' THEN ? ELSE download_started_at_ms END,
                verified_at_ms = CASE WHEN ? = 'verifying' THEN ? ELSE verified_at_ms END,
                publishing_started_at_ms = CASE WHEN ? = 'publishing' THEN ? ELSE publishing_started_at_ms END,
                remote_cleanup_started_at_ms = CASE WHEN ? = 'remote_cleanup' THEN ? ELSE remote_cleanup_started_at_ms END
             WHERE id = ? AND status = 'failed' AND version = ?",
        )
        .bind(status)
        .bind(occurred_at)
        .bind(status).bind(occurred_at)
        .bind(status).bind(occurred_at)
        .bind(status).bind(occurred_at)
        .bind(status).bind(occurred_at)
        .bind(status).bind(occurred_at)
        .bind(write.task_id.to_string())
        .bind(sqlite_u64("task_version", write.task_version)?)
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() != 1 {
            transaction.rollback().await?;
            return Ok(CasOutcome::Conflict);
        }
        let result = sqlx::query(
            "UPDATE task_attempts SET status = ?, failure_stage = NULL, failure_code = NULL,
                failure_message = NULL, failure_retryable = NULL, retry_count = 0,
                next_retry_at_ms = NULL, version = version + 1, updated_at_ms = ?
             WHERE id = ? AND status = 'failed' AND version = ?",
        )
        .bind(status)
        .bind(occurred_at)
        .bind(write.attempt.id.to_string())
        .bind(sqlite_u64("attempt_version", write.attempt.version)?)
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() != 1 {
            transaction.rollback().await?;
            return Ok(CasOutcome::Conflict);
        }
        transaction.commit().await?;
        Ok(CasOutcome::Applied {
            new_version: write.task_version + 1,
        })
    }

    pub(crate) async fn retry_processing_attempt(
        &self,
        write: &ProcessingRetryWrite,
    ) -> Result<CasOutcome, PersistenceError> {
        let mut transaction = self.database.pool().begin().await?;
        let occurred_at = timestamp(write.occurred_at);
        let attempt_number = sqlx::query(
            "UPDATE tasks SET status = 'reserved', worker_id = ?, attempt_count = attempt_count + 1,
                failure_stage = NULL, failure_code = NULL, failure_message = NULL,
                failure_retryable = NULL, retry_count = 0, next_retry_at_ms = NULL,
                cancel_requested_at_ms = NULL, reserved_at_ms = ?, version = version + 1,
                updated_at_ms = ?
             WHERE id = ? AND status = 'failed' AND version = ?
               AND EXISTS (
                   SELECT 1 FROM task_attempts
                   WHERE id = ? AND task_id = ? AND status = 'failed' AND version = ?
                     AND remote_job_id = ?
               )
               AND EXISTS (
                   SELECT 1 FROM workers WHERE id = ? AND enabled = 1 AND online = 1
               )
                AND (
                    SELECT COUNT(*) FROM tasks pending
                    WHERE pending.worker_id = ?
                      AND pending.status IN ('reserved', 'uploading', 'staged')
                ) < (SELECT MAX(worker.compute_slots - (
                        SELECT COUNT(*) FROM tasks active
                        WHERE active.worker_id = worker.id
                          AND active.status IN ('submitting', 'processing')
                    ), 0) + settings.prefetch_per_worker
                FROM workers worker
                JOIN controller_settings settings ON settings.id = 1
                WHERE worker.id = ? AND settings.paused = 0)
             RETURNING attempt_count",
        )
        .bind(write.worker_id.to_string())
        .bind(occurred_at)
        .bind(occurred_at)
        .bind(write.task_id.to_string())
        .bind(sqlite_u64("task_version", write.task_version)?)
        .bind(write.old_attempt.id.to_string())
        .bind(write.task_id.to_string())
        .bind(sqlite_u64("attempt_version", write.old_attempt.version)?)
        .bind(write.remote_job_id.to_string())
        .bind(write.worker_id.to_string())
        .bind(write.worker_id.to_string())
        .bind(write.worker_id.to_string())
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(attempt_number) = attempt_number else {
            transaction.rollback().await?;
            return Ok(CasOutcome::Conflict);
        };
        let attempt_number: i64 = attempt_number.try_get("attempt_count")?;
        sqlx::query(
            "INSERT INTO task_attempts (
                id, task_id, attempt_no, worker_id, status, submission_key,
                progress_json, created_at_ms, updated_at_ms
             ) VALUES (?, ?, ?, ?, 'reserved', ?, ?, ?, ?)",
        )
        .bind(write.new_attempt_id.to_string())
        .bind(write.task_id.to_string())
        .bind(attempt_number)
        .bind(write.worker_id.to_string())
        .bind(write.submission_key.to_string())
        .bind(encode_json("progress_json", &empty_progress())?)
        .bind(occurred_at)
        .bind(occurred_at)
        .execute(&mut *transaction)
        .await?;
        sqlx::query("UPDATE workers SET last_assigned_at_ms = ?, updated_at_ms = ? WHERE id = ?")
            .bind(occurred_at)
            .bind(occurred_at)
            .bind(write.worker_id.to_string())
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(CasOutcome::Applied {
            new_version: write.task_version + 1,
        })
    }
}
