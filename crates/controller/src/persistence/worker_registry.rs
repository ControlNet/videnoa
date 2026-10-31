use std::collections::HashMap;

use sqlx::Row;

use crate::domain::{WorkerApiUrl, WorkerCapacity, WorkerId, WorkerName};

use super::codec::{parse_brand, rust_u32, rust_u64, sqlite_u64};
use super::models::{WorkerDeleteOutcome, WorkerIdentityConflict};
use super::status_sql::{COMPUTE, TERMINAL};
use super::{PersistenceError, Store};

impl Store {
    /// Returns the duplicate normalized worker identity, if any.
    ///
    /// # Errors
    /// Returns an error when `SQLite` cannot inspect worker identities.
    pub async fn worker_identity_conflict(
        &self,
        id: Option<WorkerId>,
        name: &WorkerName,
        api_url: &WorkerApiUrl,
    ) -> Result<Option<WorkerIdentityConflict>, PersistenceError> {
        let excluded = id.map(|value| value.to_string());
        let row = sqlx::query(
            "SELECT
                EXISTS(SELECT 1 FROM workers
                    WHERE lower(trim(name)) = lower(trim(?)) AND (? IS NULL OR id != ?)) AS name_used,
                EXISTS(SELECT 1 FROM workers
                    WHERE api_url = ? AND (? IS NULL OR id != ?)) AS url_used",
        )
        .bind(name.as_str())
        .bind(excluded.as_deref())
        .bind(excluded.as_deref())
        .bind(api_url.as_url().as_str())
        .bind(excluded.as_deref())
        .bind(excluded.as_deref())
        .fetch_one(self.database.pool())
        .await?;
        if row.try_get::<i64, _>("name_used")? == 1 {
            return Ok(Some(WorkerIdentityConflict::Name));
        }
        if row.try_get::<i64, _>("url_used")? == 1 {
            return Ok(Some(WorkerIdentityConflict::ApiUrl));
        }
        Ok(None)
    }

    /// Deletes a worker whose tasks are all terminal, using optimistic concurrency.
    ///
    /// The finished tasks and attempts that ran on it are detached in the same
    /// transaction, and the attempts keep the worker's name. Tasks a retry
    /// queued for this worker lose that pin (`ON DELETE SET NULL`).
    ///
    /// # Errors
    /// Returns an error when `SQLite` cannot complete the delete policy.
    pub async fn delete_worker(
        &self,
        id: WorkerId,
        expected_version: u64,
    ) -> Result<WorkerDeleteOutcome, PersistenceError> {
        let id = id.to_string();
        let version = sqlite_u64("expected_version", expected_version)?;
        let deletable = format!(
            "EXISTS (SELECT 1 FROM workers WHERE id = ? AND version = ?)
             AND NOT EXISTS (SELECT 1 FROM tasks
                 WHERE worker_id = ? AND status NOT IN {TERMINAL})
             AND NOT EXISTS (SELECT 1 FROM task_attempts
                 WHERE worker_id = ? AND status NOT IN {TERMINAL})",
            TERMINAL = *TERMINAL,
        );
        let mut transaction = self.database.pool().begin().await?;
        // Write first, so the guard and the detach serialize with lifecycle writes.
        sqlx::query(&format!(
            "UPDATE task_attempts
             SET worker_name = (SELECT name FROM workers WHERE id = ?), worker_id = NULL
             WHERE worker_id = ? AND {deletable}"
        ))
        .bind(&id)
        .bind(&id)
        .bind(&id)
        .bind(version)
        .bind(&id)
        .bind(&id)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(&format!(
            "UPDATE tasks SET worker_id = NULL WHERE worker_id = ? AND {deletable}"
        ))
        .bind(&id)
        .bind(&id)
        .bind(version)
        .bind(&id)
        .bind(&id)
        .execute(&mut *transaction)
        .await?;
        let deleted = sqlx::query(&format!("DELETE FROM workers WHERE id = ? AND {deletable}"))
            .bind(&id)
            .bind(&id)
            .bind(version)
            .bind(&id)
            .bind(&id)
            .execute(&mut *transaction)
            .await?
            .rows_affected();
        if deleted == 1 {
            transaction.commit().await?;
            return Ok(WorkerDeleteOutcome::Deleted);
        }
        transaction.rollback().await?;
        let row = sqlx::query(&format!(
            "SELECT version,
                EXISTS(SELECT 1 FROM tasks
                    WHERE worker_id = ? AND status NOT IN {TERMINAL}) OR
                EXISTS(SELECT 1 FROM task_attempts
                    WHERE worker_id = ? AND status NOT IN {TERMINAL}) AS referenced
             FROM workers WHERE id = ?",
            TERMINAL = *TERMINAL,
        ))
        .bind(&id)
        .bind(&id)
        .bind(&id)
        .fetch_optional(self.database.pool())
        .await?;
        let Some(row) = row else {
            return Ok(WorkerDeleteOutcome::NotFound);
        };
        if rust_u64("version", row.try_get("version")?)? != expected_version {
            return Ok(WorkerDeleteOutcome::Conflict);
        }
        if row.try_get::<i64, _>("referenced")? == 1 {
            return Ok(WorkerDeleteOutcome::Referenced);
        }
        Ok(WorkerDeleteOutcome::Conflict)
    }

    /// Computes durable worker capacity from assigned task rows.
    ///
    /// # Errors
    /// Returns an error when `SQLite` access or count conversion fails.
    pub async fn worker_capacity(&self, id: WorkerId) -> Result<WorkerCapacity, PersistenceError> {
        let sql = format!("{} WHERE w.id = ? GROUP BY w.id", *CAPACITY_SELECT);
        let row = sqlx::query(&sql)
            .bind(id.to_string())
            .fetch_one(self.database.pool())
            .await?;
        map_capacity(&row)
    }

    /// Computes durable capacity for every worker in one grouped query.
    ///
    /// # Errors
    /// Returns an error when `SQLite` access, identifier decoding, or count conversion fails.
    pub async fn worker_capacities(
        &self,
    ) -> Result<HashMap<WorkerId, WorkerCapacity>, PersistenceError> {
        let sql = format!("{} GROUP BY w.id", *CAPACITY_SELECT);
        sqlx::query(&sql)
            .fetch_all(self.database.pool())
            .await?
            .iter()
            .map(|row| {
                let id = parse_brand::<WorkerId>("id", row.try_get("id")?)?;
                Ok((id, map_capacity(row)?))
            })
            .collect()
    }
}

static CAPACITY_SELECT: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    format!(
        "SELECT w.id, w.compute_slots,
    SUM(CASE WHEN t.status IN {COMPUTE} THEN 1 ELSE 0 END) AS used,
    SUM(CASE WHEN t.status NOT IN {TERMINAL} THEN 1 ELSE 0 END) AS assigned,
    SUM(CASE WHEN t.status = 'staged' THEN 1 ELSE 0 END) AS staged,
    SUM(CASE WHEN t.status = 'processing' THEN 1 ELSE 0 END) AS processing,
    SUM(CASE WHEN t.status = 'uploading' THEN 1 ELSE 0 END) AS uploads,
    SUM(CASE WHEN t.status = 'downloading' THEN 1 ELSE 0 END) AS downloads
    FROM workers w LEFT JOIN tasks t ON t.worker_id = w.id",
        COMPUTE = *COMPUTE,
        TERMINAL = *TERMINAL,
    )
});

fn map_capacity(row: &sqlx::sqlite::SqliteRow) -> Result<WorkerCapacity, PersistenceError> {
    let total = rust_u64("compute_slots", row.try_get("compute_slots")?)?;
    let used = rust_u64("used_slots", row.try_get("used")?)?;
    Ok(WorkerCapacity {
        used_slots: u16::try_from(used).map_err(|_| super::codec::corrupt("used_slots", used))?,
        available_slots: u16::try_from(total.saturating_sub(used))
            .map_err(|_| super::codec::corrupt("available_slots", total))?,
        assigned_tasks: rust_u32("assigned_tasks", row.try_get("assigned")?)?,
        staged_tasks: rust_u32("staged_tasks", row.try_get("staged")?)?,
        processing_tasks: rust_u32("processing_tasks", row.try_get("processing")?)?,
        active_uploads: u16::try_from(rust_u64("active_uploads", row.try_get("uploads")?)?)
            .map_err(|_| super::codec::corrupt("active_uploads", "overflow"))?,
        active_downloads: u16::try_from(rust_u64("active_downloads", row.try_get("downloads")?)?)
            .map_err(|_| super::codec::corrupt("active_downloads", "overflow"))?,
        progress: None,
    })
}
