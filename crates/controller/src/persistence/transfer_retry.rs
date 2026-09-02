use crate::lifecycle::TransferRetryWrite;

use super::codec::{sqlite_u64, task_status, timestamp};
use super::{CasOutcome, PersistenceError, Store};

impl Store {
    pub(crate) async fn schedule_transfer_retry(
        &self,
        write: &TransferRetryWrite,
    ) -> Result<CasOutcome, PersistenceError> {
        let mut transaction = self.database.pool().begin().await?;
        let retry_at = write.retry.next_retry_at.map(timestamp);
        let occurred_at = timestamp(write.occurred_at);
        let task = sqlx::query(
            "UPDATE tasks SET retry_count = ?, next_retry_at_ms = ?,
                version = version + 1, updated_at_ms = ?
             WHERE id = ? AND status = ? AND version = ?",
        )
        .bind(i64::from(write.retry.retry_count))
        .bind(retry_at)
        .bind(occurred_at)
        .bind(write.task_id.to_string())
        .bind(task_status(write.attempt.status))
        .bind(sqlite_u64("task_version", write.task_version)?)
        .execute(&mut *transaction)
        .await?;
        if task.rows_affected() != 1 {
            transaction.rollback().await?;
            return Ok(CasOutcome::Conflict);
        }
        let attempt = sqlx::query(
            "UPDATE task_attempts SET retry_count = ?, next_retry_at_ms = ?,
                version = version + 1, updated_at_ms = ?
             WHERE id = ? AND status = ? AND version = ?",
        )
        .bind(i64::from(write.retry.retry_count))
        .bind(retry_at)
        .bind(occurred_at)
        .bind(write.attempt.id.to_string())
        .bind(task_status(write.attempt.status))
        .bind(sqlite_u64("attempt_version", write.attempt.version)?)
        .execute(&mut *transaction)
        .await?;
        if attempt.rows_affected() != 1 {
            transaction.rollback().await?;
            return Ok(CasOutcome::Conflict);
        }
        transaction.commit().await?;
        Ok(CasOutcome::Applied {
            new_version: write.task_version + 1,
        })
    }
}
