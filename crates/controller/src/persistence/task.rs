use sqlx::Row;

use super::codec::{encode_json, sqlite_u64, task_source, timestamp};
use super::models::{empty_progress, NewTask, TaskRecord};
use super::task_row::{map_task, TASK_COLUMNS};
use super::{PersistenceError, Store};

impl Store {
    /// # Errors
    /// Returns an error when `SQLite` access or task encoding fails.
    pub async fn insert_task(&self, task: &NewTask) -> Result<(), PersistenceError> {
        let mut connection = self.database.pool().acquire().await?;
        insert_task_on(&mut connection, task).await
    }

    /// # Errors
    /// Returns an error when `SQLite` access or persisted decoding fails.
    pub async fn task(
        &self,
        id: crate::domain::TaskId,
    ) -> Result<Option<TaskRecord>, PersistenceError> {
        let sql = format!("SELECT {TASK_COLUMNS} FROM tasks WHERE id = ?");
        sqlx::query(&sql)
            .bind(id.to_string())
            .fetch_optional(self.database.pool())
            .await?
            .as_ref()
            .map(map_task)
            .transpose()
    }

    /// # Errors
    /// Returns an error when `SQLite` access or persisted decoding fails.
    pub async fn recovery_tasks(&self, limit: u16) -> Result<Vec<TaskRecord>, PersistenceError> {
        let sql = format!(
            "SELECT {TASK_COLUMNS} FROM tasks
             WHERE status NOT IN ('completed', 'failed', 'cancelled')
             ORDER BY updated_at_ms ASC, id ASC LIMIT ?"
        );
        let rows = sqlx::query(&sql)
            .bind(i64::from(limit))
            .fetch_all(self.database.pool())
            .await?;
        rows.iter().map(map_task).collect()
    }

    /// # Errors
    /// Returns an error when `SQLite` access or count conversion fails.
    pub async fn count_attempts_for_tasks(
        &self,
        task_ids: &[crate::domain::TaskId],
    ) -> Result<u64, PersistenceError> {
        let mut query =
            sqlx::QueryBuilder::new("SELECT COUNT(*) FROM task_attempts WHERE task_id IN (");
        let mut separated = query.separated(", ");
        for task_id in task_ids {
            separated.push_bind(task_id.to_string());
        }
        separated.push_unseparated(")");
        let count: i64 = query
            .build()
            .fetch_one(self.database.pool())
            .await?
            .try_get(0)?;
        super::codec::rust_u64("attempt_count", count)
    }
}

pub(crate) async fn insert_task_on(
    connection: &mut sqlx::SqliteConnection,
    task: &NewTask,
) -> Result<(), PersistenceError> {
    let progress = encode_json("progress_json", &empty_progress())?;
    sqlx::query(
        "INSERT INTO tasks (
                id, status, input_path, output_path, input_extension, output_extension,
                workflow, priority, source, source_reference, input_size, input_mtime_ms,
                input_identity, input_content_identity, progress_json, created_at_ms, updated_at_ms
             ) VALUES (?, 'queued', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(task.id.to_string())
    .bind(task.request.input_path.as_str())
    .bind(task.request.output_path.as_str())
    .bind(task.input_extension.as_str())
    .bind(task.output_extension.as_str())
    .bind(task.request.workflow.as_str())
    .bind(task.request.priority)
    .bind(task_source(task.request.source))
    .bind(
        task.request
            .source_reference
            .as_ref()
            .map(crate::domain::SourceReference::as_str),
    )
    .bind(sqlite_u64("input_size", task.input_size)?)
    .bind(timestamp(task.input_mtime))
    .bind(task.input_identity.as_bytes().as_slice())
    .bind(task.input_content_identity.as_bytes().as_slice())
    .bind(progress)
    .bind(timestamp(task.created_at))
    .bind(timestamp(task.created_at))
    .execute(connection)
    .await?;
    Ok(())
}
