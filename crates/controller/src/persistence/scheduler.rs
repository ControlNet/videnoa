use sqlx::Row;

use super::codec::{parse_brand, rust_u64};
use super::models::{SchedulerCandidate, UploadCandidateRecord};
use super::{PersistenceError, Store};
use crate::domain::{TaskId, WorkerId};

impl Store {
    /// Selects the next durable task/worker pair without claiming it.
    ///
    /// # Errors
    /// Returns an error when `SQLite` cannot evaluate scheduler eligibility.
    pub async fn scheduler_candidate(
        &self,
    ) -> Result<Option<SchedulerCandidate>, PersistenceError> {
        let row = sqlx::query(SCHEDULER_CANDIDATE_SQL)
            .fetch_optional(self.database.pool())
            .await?;
        row.as_ref().map(map_candidate).transpose()
    }

    /// Selects reserved upload work with idle-worker feeds first.
    ///
    /// # Errors
    /// Returns an error when `SQLite` cannot evaluate the upload queue.
    pub async fn upload_candidates(
        &self,
        limit: u16,
    ) -> Result<Vec<UploadCandidateRecord>, PersistenceError> {
        let rows = sqlx::query(UPLOAD_CANDIDATES_SQL)
            .bind(i64::from(limit))
            .fetch_all(self.database.pool())
            .await?;
        rows.iter().map(map_upload_candidate).collect()
    }
}

fn map_candidate(row: &sqlx::sqlite::SqliteRow) -> Result<SchedulerCandidate, PersistenceError> {
    Ok(SchedulerCandidate {
        task_id: parse_brand::<TaskId>("task_id", row.try_get("task_id")?)?,
        task_version: rust_u64("task_version", row.try_get("task_version")?)?,
        worker_id: parse_brand::<WorkerId>("worker_id", row.try_get("worker_id")?)?,
        idle_feed: row.try_get::<i64, _>("used_slots")? == 0,
    })
}

fn map_upload_candidate(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<UploadCandidateRecord, PersistenceError> {
    Ok(UploadCandidateRecord {
        task_id: parse_brand::<TaskId>("task_id", row.try_get("task_id")?)?,
        worker_id: parse_brand::<WorkerId>("worker_id", row.try_get("worker_id")?)?,
        idle_feed: row.try_get::<i64, _>("upload_rank")? == 0,
    })
}

const SCHEDULER_CANDIDATE_SQL: &str = "WITH worker_load AS (
    SELECT w.id,
        (SELECT COUNT(*) FROM tasks assigned
         WHERE assigned.worker_id = w.id
           AND assigned.status NOT IN ('completed', 'failed', 'cancelled')) AS used_slots,
        (SELECT COUNT(*) FROM tasks pending
         WHERE pending.worker_id = w.id
           AND pending.status IN ('reserved', 'uploading', 'staged')) AS pending_slots,
        (SELECT COUNT(*) FROM tasks active
         WHERE active.worker_id = w.id
           AND active.status IN ('submitting', 'processing')) AS active_compute
    FROM workers w
)
SELECT t.id AS task_id, t.version AS task_version, w.id AS worker_id, load.used_slots
FROM tasks t
JOIN workers w
JOIN worker_load load ON load.id = w.id
JOIN controller_settings settings ON settings.id = 1
WHERE t.status = 'queued' AND settings.paused = 0
  AND w.enabled = 1 AND w.online = 1 AND load.used_slots < w.compute_slots
  AND EXISTS (
      SELECT 1 FROM json_each(w.capabilities_json, '$.workflows') capability
      WHERE json_extract(capability.value, '$.name') = t.workflow
  )
  AND (
      load.used_slots = 0 OR
      load.pending_slots < settings.prefetch_per_worker + CASE WHEN load.active_compute = 0 THEN 1 ELSE 0 END
  )
ORDER BY t.priority DESC, t.created_at_ms ASC, t.id ASC,
         load.used_slots ASC, w.last_assigned_at_ms ASC, w.id ASC
LIMIT 1";

const UPLOAD_CANDIDATES_SQL: &str = "SELECT t.id AS task_id, t.worker_id,
    CASE WHEN
        NOT EXISTS (SELECT 1 FROM tasks active
            WHERE active.worker_id = t.worker_id
              AND active.status IN ('submitting', 'processing'))
        AND NOT EXISTS (SELECT 1 FROM tasks earlier
            WHERE earlier.worker_id = t.worker_id AND earlier.status = 'reserved'
              AND (earlier.priority > t.priority
                OR (earlier.priority = t.priority AND earlier.created_at_ms < t.created_at_ms)
                OR (earlier.priority = t.priority AND earlier.created_at_ms = t.created_at_ms AND earlier.id < t.id)))
        THEN 0 ELSE 1 END AS upload_rank
FROM tasks t
JOIN controller_settings settings ON settings.id = 1
WHERE t.status = 'reserved' AND settings.paused = 0
ORDER BY upload_rank ASC, t.priority DESC, t.created_at_ms ASC, t.id ASC
LIMIT ?";
