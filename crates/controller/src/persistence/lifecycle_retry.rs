use crate::lifecycle::{RequeueRetryWrite, RetryWrite};

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
                next_retry_at_ms = NULL, version = version + 1, updated_at_ms = ?,
                submission_owner = CASE WHEN ? = 'staged' THEN NULL ELSE submission_owner END
             WHERE id = ? AND status = 'failed' AND version = ?",
        )
        .bind(status)
        .bind(occurred_at)
        .bind(status)
        .bind(write.attempt.id.to_string())
        .bind(sqlite_u64("attempt_version", write.attempt.version)?)
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() != 1 {
            transaction.rollback().await?;
            return Ok(CasOutcome::Conflict);
        }
        transaction.commit().await?;
        tracing::info!(task_id = %write.task_id, attempt_id = %write.attempt.id, to = status, "Task retry requested");
        Ok(CasOutcome::Applied {
            new_version: write.task_version + 1,
        })
    }

    pub(crate) async fn requeue_failed_task(
        &self,
        write: &RequeueRetryWrite,
    ) -> Result<CasOutcome, PersistenceError> {
        let occurred_at = timestamp(write.occurred_at);
        let result = sqlx::query(
            "UPDATE tasks SET status = 'queued', worker_id = NULL, requested_worker_id = ?,
                failure_stage = NULL, failure_code = NULL, failure_message = NULL,
                failure_retryable = NULL, retry_count = 0, next_retry_at_ms = NULL,
                cancel_requested_at_ms = NULL, progress_json = ?, version = version + 1,
                updated_at_ms = ?
             WHERE id = ? AND status = 'failed' AND version = ?
               AND EXISTS (
                   SELECT 1 FROM task_attempts
                   WHERE id = ? AND task_id = ? AND status = 'failed' AND version = ?
               )",
        )
        .bind(write.requested_worker_id.to_string())
        .bind(encode_json("progress_json", &empty_progress())?)
        .bind(occurred_at)
        .bind(write.task_id.to_string())
        .bind(sqlite_u64("task_version", write.task_version)?)
        .bind(write.attempt.id.to_string())
        .bind(write.task_id.to_string())
        .bind(sqlite_u64("attempt_version", write.attempt.version)?)
        .execute(self.database.pool())
        .await?;
        if result.rows_affected() != 1 {
            return Ok(CasOutcome::Conflict);
        }
        tracing::info!(task_id = %write.task_id, failed_attempt_id = %write.attempt.id, requested_worker_id = %write.requested_worker_id, "Task requeued for retry");
        Ok(CasOutcome::Applied {
            new_version: write.task_version + 1,
        })
    }
}
