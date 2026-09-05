use sqlx::Row;

use crate::domain::{WorkerApiUrl, WorkerCapacity, WorkerId, WorkerName};

use super::codec::{rust_u32, rust_u64, sqlite_u64};
use super::models::{WorkerDeleteOutcome, WorkerIdentityConflict};
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

    /// Deletes an unreferenced worker using optimistic concurrency.
    ///
    /// # Errors
    /// Returns an error when `SQLite` cannot complete the delete policy.
    pub async fn delete_worker(
        &self,
        id: WorkerId,
        expected_version: u64,
    ) -> Result<WorkerDeleteOutcome, PersistenceError> {
        let result = sqlx::query(
            "DELETE FROM workers
             WHERE id = ? AND version = ?
               AND NOT EXISTS (SELECT 1 FROM tasks WHERE worker_id = ?)
               AND NOT EXISTS (SELECT 1 FROM task_attempts WHERE worker_id = ?)",
        )
        .bind(id.to_string())
        .bind(sqlite_u64("expected_version", expected_version)?)
        .bind(id.to_string())
        .bind(id.to_string())
        .execute(self.database.pool())
        .await?;
        if result.rows_affected() == 1 {
            return Ok(WorkerDeleteOutcome::Deleted);
        }
        let row = sqlx::query(
            "SELECT version,
                EXISTS(SELECT 1 FROM tasks WHERE worker_id = ?) OR
                EXISTS(SELECT 1 FROM task_attempts WHERE worker_id = ?) AS referenced
             FROM workers WHERE id = ?",
        )
        .bind(id.to_string())
        .bind(id.to_string())
        .bind(id.to_string())
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
        let row = sqlx::query(
            "SELECT w.compute_slots,
                SUM(CASE WHEN t.status IN ('submitting', 'processing') THEN 1 ELSE 0 END) AS used,
                SUM(CASE WHEN t.status NOT IN ('completed', 'failed', 'cancelled') THEN 1 ELSE 0 END) AS assigned,
                SUM(CASE WHEN t.status = 'staged' THEN 1 ELSE 0 END) AS staged,
                SUM(CASE WHEN t.status = 'processing' THEN 1 ELSE 0 END) AS processing,
                SUM(CASE WHEN t.status = 'uploading' THEN 1 ELSE 0 END) AS uploads,
                SUM(CASE WHEN t.status = 'downloading' THEN 1 ELSE 0 END) AS downloads
             FROM workers w LEFT JOIN tasks t ON t.worker_id = w.id
             WHERE w.id = ? GROUP BY w.id",
        )
        .bind(id.to_string())
        .fetch_one(self.database.pool())
        .await?;
        let total = rust_u64("compute_slots", row.try_get("compute_slots")?)?;
        let used = rust_u64("used_slots", row.try_get("used")?)?;
        Ok(WorkerCapacity {
            used_slots: u16::try_from(used)
                .map_err(|_| super::codec::corrupt("used_slots", used))?,
            available_slots: u16::try_from(total.saturating_sub(used))
                .map_err(|_| super::codec::corrupt("available_slots", total))?,
            assigned_tasks: rust_u32("assigned_tasks", row.try_get("assigned")?)?,
            staged_tasks: rust_u32("staged_tasks", row.try_get("staged")?)?,
            processing_tasks: rust_u32("processing_tasks", row.try_get("processing")?)?,
            active_uploads: u16::try_from(rust_u64("active_uploads", row.try_get("uploads")?)?)
                .map_err(|_| super::codec::corrupt("active_uploads", "overflow"))?,
            active_downloads: u16::try_from(rust_u64(
                "active_downloads",
                row.try_get("downloads")?,
            )?)
            .map_err(|_| super::codec::corrupt("active_downloads", "overflow"))?,
            progress: None,
        })
    }
}
