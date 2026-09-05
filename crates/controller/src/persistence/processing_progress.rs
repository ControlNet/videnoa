use chrono::{DateTime, Utc};

use crate::domain::TaskProgress;

use super::codec::{encode_json, sqlite_u64, timestamp};
use super::{AttemptRecord, CasOutcome, DurableChange, PersistenceError, Store, TaskRecord};

impl Store {
    pub(crate) async fn record_processing_progress(
        &self,
        task: &TaskRecord,
        attempt: &AttemptRecord,
        progress: &TaskProgress,
        now: DateTime<Utc>,
    ) -> Result<CasOutcome, PersistenceError> {
        let json = encode_json("progress_json", progress)?;
        let mut transaction = self.database.pool().begin().await?;
        let changed = sqlx::query(
            "UPDATE tasks SET progress_json = ?, version = version + 1, updated_at_ms = ?
             WHERE id = ? AND version = ? AND status = 'processing'
               AND cancel_requested_at_ms IS NULL AND attempt_count = ?",
        )
        .bind(&json)
        .bind(timestamp(now))
        .bind(task.id.to_string())
        .bind(sqlite_u64("task_version", task.version)?)
        .bind(i64::from(attempt.attempt.attempt_number))
        .execute(&mut *transaction)
        .await?;
        if changed.rows_affected() != 1 {
            transaction.rollback().await?;
            return Ok(CasOutcome::Conflict);
        }
        let changed = sqlx::query(
            "UPDATE task_attempts SET progress_json = ?, version = version + 1, updated_at_ms = ?
             WHERE id = ? AND task_id = ? AND version = ? AND status = 'processing'",
        )
        .bind(&json)
        .bind(timestamp(now))
        .bind(attempt.attempt.id.to_string())
        .bind(task.id.to_string())
        .bind(sqlite_u64("attempt_version", attempt.version)?)
        .execute(&mut *transaction)
        .await?;
        if changed.rows_affected() != 1 {
            transaction.rollback().await?;
            return Ok(CasOutcome::Conflict);
        }
        transaction.commit().await?;
        self.notify_change(DurableChange::Task(task.id));
        Ok(CasOutcome::Applied {
            new_version: task.version + 1,
        })
    }
}
