use sqlx::Row;

use crate::domain::{PageRequest, TaskId};

use super::attempt::{map_attempt, ATTEMPT_SELECT_ALL};
use super::{AttemptRecord, PageResult, PersistenceError, Store};

impl Store {
    /// Loads one stable bounded page of attempts for a task.
    ///
    /// # Errors
    /// Returns an error when `SQLite` access, count conversion, or persisted decoding fails.
    pub async fn task_attempt_page(
        &self,
        task_id: TaskId,
        page: PageRequest,
    ) -> Result<PageResult<AttemptRecord>, PersistenceError> {
        let mut transaction = self.database.pool().begin().await?;
        let count: i64 = sqlx::query("SELECT COUNT(*) FROM task_attempts WHERE task_id = ?")
            .bind(task_id.to_string())
            .fetch_one(&mut *transaction)
            .await?
            .try_get(0)?;
        let sql = format!(
            "{ATTEMPT_SELECT_ALL} AND task_id = ? \
             ORDER BY attempt_no DESC, id DESC LIMIT ? OFFSET ?"
        );
        let rows = sqlx::query(&sql)
            .bind(task_id.to_string())
            .bind(i64::from(page.limit().get()))
            .bind(super::codec::sqlite_u64(
                "attempt_offset",
                page.offset().get(),
            )?)
            .fetch_all(&mut *transaction)
            .await?;
        transaction.commit().await?;
        let total = super::codec::rust_u64("task_attempt_count", count)?;
        let items = rows
            .iter()
            .map(map_attempt)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(PageResult { items, total })
    }

    /// Loads the newest attempts for an internal lifecycle caller.
    ///
    /// # Errors
    /// Returns an error when `SQLite` access or persisted decoding fails.
    pub async fn task_attempts(
        &self,
        task_id: TaskId,
        limit: u16,
    ) -> Result<Vec<AttemptRecord>, PersistenceError> {
        let sql = format!(
            "{ATTEMPT_SELECT_ALL} AND task_id = ? ORDER BY attempt_no DESC, id DESC LIMIT ?"
        );
        let rows = sqlx::query(&sql)
            .bind(task_id.to_string())
            .bind(i64::from(limit))
            .fetch_all(self.database.pool())
            .await?;
        rows.iter().map(map_attempt).collect()
    }

    /// Loads the newest durable attempt for one task.
    ///
    /// # Errors
    /// Returns an error when `SQLite` access or persisted decoding fails.
    pub async fn current_attempt(
        &self,
        task_id: TaskId,
    ) -> Result<Option<AttemptRecord>, PersistenceError> {
        let sql = format!(
            "{ATTEMPT_SELECT_ALL} AND task_id = ? ORDER BY attempt_no DESC, id DESC LIMIT 1"
        );
        sqlx::query(&sql)
            .bind(task_id.to_string())
            .fetch_optional(self.database.pool())
            .await?
            .as_ref()
            .map(map_attempt)
            .transpose()
    }
}
