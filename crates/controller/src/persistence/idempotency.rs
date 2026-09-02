use sqlx::Row;

use crate::domain::{IdempotencyKey, TaskId};

use super::codec::{parse_brand, parse_timestamp, timestamp};
use super::models::{IdempotencyRecord, NewTask, TaskIngressOutcome};
use super::task::insert_task_on;
use super::{PersistenceError, Store};

impl Store {
    /// # Errors
    /// Returns an error when the atomic `SQLite` transaction cannot complete.
    pub async fn insert_task_with_idempotency(
        &self,
        task: &NewTask,
        record: &IdempotencyRecord,
    ) -> Result<TaskIngressOutcome, PersistenceError> {
        let mut transaction = self.database.pool().begin().await?;
        insert_task_on(&mut transaction, task).await?;
        let inserted = sqlx::query(
            "INSERT INTO task_idempotency (
                idempotency_key, request_fingerprint, task_id, created_at_ms
             ) VALUES (?, ?, ?, ?) ON CONFLICT(idempotency_key) DO NOTHING",
        )
        .bind(record.key.as_str())
        .bind(record.request_fingerprint.as_slice())
        .bind(task.id.to_string())
        .bind(timestamp(record.created_at))
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if inserted == 1 {
            transaction.commit().await?;
            return Ok(TaskIngressOutcome::Inserted);
        }
        let existing = sqlx::query(
            "SELECT request_fingerprint, task_id FROM task_idempotency WHERE idempotency_key = ?",
        )
        .bind(record.key.as_str())
        .fetch_one(&mut *transaction)
        .await?;
        let fingerprint: Vec<u8> = existing.try_get("request_fingerprint")?;
        let existing_task = parse_brand::<TaskId>("task_id", existing.try_get("task_id")?)?;
        transaction.rollback().await?;
        if fingerprint.as_slice() == record.request_fingerprint {
            Ok(TaskIngressOutcome::Replay(existing_task))
        } else {
            Ok(TaskIngressOutcome::Conflict)
        }
    }

    /// # Errors
    /// Returns an error when `SQLite` cannot persist the idempotency record.
    pub async fn insert_task_idempotency(
        &self,
        record: &IdempotencyRecord,
    ) -> Result<(), PersistenceError> {
        sqlx::query(
            "INSERT INTO task_idempotency (
                idempotency_key, request_fingerprint, task_id, created_at_ms
             ) VALUES (?, ?, ?, ?)",
        )
        .bind(record.key.as_str())
        .bind(record.request_fingerprint.as_slice())
        .bind(record.task_id.to_string())
        .bind(timestamp(record.created_at))
        .execute(self.database.pool())
        .await?;
        Ok(())
    }

    /// # Errors
    /// Returns an error when `SQLite` access or persisted decoding fails.
    pub async fn task_idempotency(
        &self,
        key: &IdempotencyKey,
    ) -> Result<Option<IdempotencyRecord>, PersistenceError> {
        let row = sqlx::query(
            "SELECT idempotency_key, request_fingerprint, task_id, created_at_ms
             FROM task_idempotency WHERE idempotency_key = ?",
        )
        .bind(key.as_str())
        .fetch_optional(self.database.pool())
        .await?;
        row.as_ref().map(map_record).transpose()
    }
}

fn map_record(row: &sqlx::sqlite::SqliteRow) -> Result<IdempotencyRecord, PersistenceError> {
    let fingerprint: Vec<u8> = row.try_get("request_fingerprint")?;
    let request_fingerprint = <[u8; 32]>::try_from(fingerprint)
        .map_err(|bytes| super::codec::corrupt("request_fingerprint", bytes.len()))?;
    Ok(IdempotencyRecord {
        key: IdempotencyKey::new(row.try_get::<String, _>("idempotency_key")?),
        request_fingerprint,
        task_id: parse_brand::<TaskId>("task_id", row.try_get("task_id")?)?,
        created_at: parse_timestamp("created_at_ms", row.try_get("created_at_ms")?)?,
    })
}
