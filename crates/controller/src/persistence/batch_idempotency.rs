use sqlx::{Row, Sqlite, Transaction};

use crate::domain::IdempotencyKey;

use super::{NewTask, PersistenceError, Store, TaskRecord};

pub(crate) struct BatchReceipt {
    pub fingerprint: Vec<u8>,
    pub status: u16,
    pub body: String,
}

pub(crate) enum BatchAdmission<'a> {
    Fresh(Transaction<'a, Sqlite>),
    Existing(BatchReceipt),
}

impl Store {
    pub(crate) async fn batch_receipt(
        &self,
        key: &IdempotencyKey,
    ) -> Result<Option<BatchReceipt>, PersistenceError> {
        let row = sqlx::query("SELECT request_fingerprint, response_status, response_json FROM batch_idempotency WHERE idempotency_key = ?")
            .bind(key.as_str()).fetch_optional(self.database.pool()).await?;
        row.as_ref().map(receipt).transpose()
    }

    pub(crate) async fn begin_batch(
        &self,
        key: &IdempotencyKey,
        fingerprint: &[u8; 32],
    ) -> Result<BatchAdmission<'_>, PersistenceError> {
        let mut transaction = self.database.pool().begin().await?;
        // Write first: concurrent claimants serialize without a stale read snapshot.
        let inserted = sqlx::query("INSERT INTO batch_idempotency (idempotency_key, request_fingerprint, created_at_ms) VALUES (?, ?, ?) ON CONFLICT(idempotency_key) DO NOTHING")
            .bind(key.as_str()).bind(fingerprint.as_slice()).bind(chrono::Utc::now().timestamp_millis())
            .execute(&mut *transaction).await?.rows_affected();
        if inserted == 1 {
            return Ok(BatchAdmission::Fresh(transaction));
        }
        let row = sqlx::query("SELECT request_fingerprint, response_status, response_json FROM batch_idempotency WHERE idempotency_key = ?")
            .bind(key.as_str()).fetch_one(&mut *transaction).await?;
        let record = receipt(&row)?;
        transaction.rollback().await?;
        Ok(BatchAdmission::Existing(record))
    }
}

fn receipt(row: &sqlx::sqlite::SqliteRow) -> Result<BatchReceipt, PersistenceError> {
    let status: i64 = row.try_get("response_status")?;
    Ok(BatchReceipt {
        fingerprint: row.try_get("request_fingerprint")?,
        status: u16::try_from(status)
            .map_err(|_| super::codec::corrupt("response_status", status))?,
        body: row.try_get("response_json")?,
    })
}

pub(crate) async fn insert_batch_task(
    connection: &mut sqlx::SqliteConnection,
    task: &NewTask,
) -> Result<TaskRecord, PersistenceError> {
    super::task::insert_task_on(connection, task).await?;
    let sql = format!(
        "SELECT {} FROM tasks WHERE id = ?",
        super::task_row::TASK_COLUMNS
    );
    let row = sqlx::query(&sql)
        .bind(task.id.to_string())
        .fetch_one(connection)
        .await?;
    super::task_row::map_task(&row)
}

pub(crate) async fn finish_batch(
    mut transaction: Transaction<'_, Sqlite>,
    key: &IdempotencyKey,
    status: u16,
    body: &str,
) -> Result<(), PersistenceError> {
    sqlx::query("UPDATE batch_idempotency SET response_status = ?, response_json = ? WHERE idempotency_key = ?")
        .bind(i64::from(status)).bind(body).bind(key.as_str()).execute(&mut *transaction).await?;
    transaction.commit().await?;
    Ok(())
}
