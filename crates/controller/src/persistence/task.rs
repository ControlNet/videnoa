use sqlx::Row;

use super::codec::{encode_json, sqlite_u64, task_source, task_status, timestamp};
use super::models::{empty_progress, CasOutcome, NewTask, TaskRecord, TaskTransition};
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
    /// Returns an error when `SQLite` access or value encoding fails.
    pub async fn transition_task(
        &self,
        transition: &TaskTransition,
    ) -> Result<CasOutcome, PersistenceError> {
        let occurred_at = timestamp(transition.occurred_at);
        let result = sqlx::query(
            "UPDATE tasks SET
                status = ?, version = version + 1, updated_at_ms = ?,
                reserved_at_ms = CASE WHEN ? = 'reserved' THEN ? ELSE reserved_at_ms END,
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
        .bind(task_status(transition.next_status))
        .bind(occurred_at)
        .bind(task_status(transition.next_status)).bind(occurred_at)
        .bind(task_status(transition.next_status)).bind(occurred_at)
        .bind(task_status(transition.next_status)).bind(occurred_at)
        .bind(task_status(transition.next_status)).bind(occurred_at)
        .bind(task_status(transition.next_status)).bind(occurred_at)
        .bind(task_status(transition.next_status)).bind(occurred_at)
        .bind(task_status(transition.next_status)).bind(occurred_at)
        .bind(task_status(transition.next_status)).bind(occurred_at)
        .bind(task_status(transition.next_status)).bind(occurred_at)
        .bind(task_status(transition.next_status)).bind(occurred_at)
        .bind(task_status(transition.next_status)).bind(occurred_at)
        .bind(transition.task_id.to_string())
        .bind(task_status(transition.expected_status))
        .bind(sqlite_u64("expected_version", transition.expected_version)?)
        .execute(self.database.pool())
        .await?;
        if result.rows_affected() == 1 {
            Ok(CasOutcome::Applied {
                new_version: transition.expected_version + 1,
            })
        } else {
            Ok(CasOutcome::Conflict)
        }
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
                input_identity, progress_json, created_at_ms, updated_at_ms
             ) VALUES (?, 'queued', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
    .bind(progress)
    .bind(timestamp(task.created_at))
    .bind(timestamp(task.created_at))
    .execute(connection)
    .await?;
    Ok(())
}
