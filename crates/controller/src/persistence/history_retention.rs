use chrono::{DateTime, Utc};

use crate::domain::TaskId;

use super::codec::{sqlite_u64, timestamp};
use super::status_sql::TERMINAL;
use super::task_row::{map_task, TASK_COLUMNS};
use super::{PersistenceError, Store, TaskRecord};

impl Store {
    /// Deletes batch `Idempotency-Key` receipts created before `created_before`.
    pub(crate) async fn expire_batch_receipts(
        &self,
        created_before: DateTime<Utc>,
    ) -> Result<u64, PersistenceError> {
        Ok(
            sqlx::query("DELETE FROM batch_idempotency WHERE created_at_ms < ?")
                .bind(timestamp(created_before))
                .execute(self.database.pool())
                .await?
                .rows_affected(),
        )
    }

    /// Lists finished tasks last changed before `updated_before`, in id order
    /// after `after`.
    pub(crate) async fn finished_tasks_updated_before(
        &self,
        updated_before: DateTime<Utc>,
        after: Option<TaskId>,
        limit: u32,
    ) -> Result<Vec<TaskRecord>, PersistenceError> {
        let sql = format!(
            "SELECT {TASK_COLUMNS} FROM tasks
             WHERE status IN {terminal} AND updated_at_ms < ? AND id > ?
             ORDER BY id LIMIT ?",
            terminal = *TERMINAL,
        );
        let rows = sqlx::query(&sql)
            .bind(timestamp(updated_before))
            .bind(after.map_or_else(String::new, |id| id.to_string()))
            .bind(i64::from(limit))
            .fetch_all(self.database.pool())
            .await?;
        rows.iter().map(map_task).collect()
    }

    /// Deletes a finished task with its attempts and idempotency key, unless
    /// it changed since `version` was read. Returns whether it was deleted.
    pub(crate) async fn purge_finished_task(
        &self,
        task_id: TaskId,
        version: u64,
    ) -> Result<bool, PersistenceError> {
        let id = task_id.to_string();
        let version = sqlite_u64("task_version", version)?;
        let unchanged = format!(
            "EXISTS (SELECT 1 FROM tasks WHERE id = ? AND version = ? AND status IN {})",
            *TERMINAL
        );
        let mut transaction = self.database.pool().begin().await?;
        // Write first: the version check and the deletes see one snapshot.
        sqlx::query(&format!(
            "DELETE FROM task_idempotency WHERE task_id = ? AND {unchanged}"
        ))
        .bind(&id)
        .bind(&id)
        .bind(version)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(&format!(
            "DELETE FROM task_attempts WHERE task_id = ? AND {unchanged}"
        ))
        .bind(&id)
        .bind(&id)
        .bind(version)
        .execute(&mut *transaction)
        .await?;
        let deleted = sqlx::query(&format!(
            "DELETE FROM tasks WHERE id = ? AND version = ? AND status IN {}",
            *TERMINAL
        ))
        .bind(&id)
        .bind(version)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if deleted != 1 {
            transaction.rollback().await?;
            return Ok(false);
        }
        transaction.commit().await?;
        tracing::info!(%task_id, "Expired task history purged");
        Ok(true)
    }
}
