use sqlx::Row;

use crate::domain::{ComputeSlots, WorkerApiUrl, WorkerCapabilities, WorkerId, WorkerName};

use super::codec::{
    boolean, decode_json, encode_json, parse_brand, parse_optional_timestamp, parse_timestamp,
    rust_u32, rust_u64, sqlite_u64, timestamp,
};
use super::models::{
    CasOutcome, NewWorker, WorkerHealthUpdate, WorkerRecord, WorkerUpdate, WorkerUpdateOutcome,
};
use super::{PersistenceError, Store};

impl Store {
    /// # Errors
    /// Returns an error when `SQLite` access or capability encoding fails.
    pub async fn insert_worker(&self, worker: &NewWorker) -> Result<(), PersistenceError> {
        let capabilities = WorkerCapabilities {
            workflows: Vec::new(),
            refreshed_at: None,
        };
        sqlx::query(
            "INSERT INTO workers (
                id, name, api_url, enabled, online, compute_slots, capabilities_json,
                created_at_ms, updated_at_ms
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(worker.id.to_string())
        .bind(worker.name.as_str())
        .bind(worker.api_url.as_url().as_str())
        .bind(worker.enabled)
        .bind(worker.online)
        .bind(i64::from(worker.compute_slots.get()))
        .bind(encode_json("capabilities_json", &capabilities)?)
        .bind(timestamp(worker.created_at))
        .bind(timestamp(worker.created_at))
        .execute(self.database.pool())
        .await?;
        Ok(())
    }

    /// # Errors
    /// Returns an error when `SQLite` access or persisted decoding fails.
    pub async fn worker(&self, id: WorkerId) -> Result<Option<WorkerRecord>, PersistenceError> {
        let sql = format!("{WORKER_COLUMNS} WHERE id = ?");
        sqlx::query(&sql)
            .bind(id.to_string())
            .fetch_optional(self.database.pool())
            .await?
            .as_ref()
            .map(map_worker)
            .transpose()
    }

    /// # Errors
    /// Returns an error when `SQLite` access or value encoding fails.
    pub async fn update_worker(
        &self,
        update: &WorkerUpdate,
    ) -> Result<WorkerUpdateOutcome, PersistenceError> {
        let mut transaction = self.database.pool().begin().await?;
        let updated_version: Option<i64> = sqlx::query_scalar(
            "UPDATE workers SET name = ?, api_url = ?, enabled = ?, compute_slots = ?,
                version = version + 1, updated_at_ms = ?
             WHERE id = ? AND version = ?
               AND (SELECT COUNT(*) FROM tasks assigned
                    WHERE assigned.worker_id = workers.id
                      AND assigned.status NOT IN ('completed', 'failed', 'cancelled')) <= ?
             RETURNING version",
        )
        .bind(update.name.as_str())
        .bind(update.api_url.as_url().as_str())
        .bind(update.enabled)
        .bind(i64::from(update.compute_slots.get()))
        .bind(timestamp(update.updated_at))
        .bind(update.id.to_string())
        .bind(sqlite_u64("expected_version", update.expected_version)?)
        .bind(i64::from(update.compute_slots.get()))
        .fetch_optional(&mut *transaction)
        .await?;
        let outcome = if let Some(version) = updated_version {
            WorkerUpdateOutcome::Applied {
                new_version: rust_u64("version", version)?,
            }
        } else {
            let state: Option<(i64, i64)> = sqlx::query_as(
                "SELECT worker.version,
                    (SELECT COUNT(*) FROM tasks assigned
                     WHERE assigned.worker_id = worker.id
                       AND assigned.status NOT IN ('completed', 'failed', 'cancelled'))
                 FROM workers worker WHERE worker.id = ?",
            )
            .bind(update.id.to_string())
            .fetch_optional(&mut *transaction)
            .await?;
            match state {
                Some((version, used_slots))
                    if rust_u64("version", version)? == update.expected_version
                        && rust_u64("used_slots", used_slots)?
                            > u64::from(update.compute_slots.get()) =>
                {
                    WorkerUpdateOutcome::CapacityBelowUsage
                }
                Some(_) | None => WorkerUpdateOutcome::Conflict,
            }
        };
        transaction.commit().await?;
        Ok(outcome)
    }

    /// # Errors
    /// Returns an error when `SQLite` access or health encoding fails.
    pub async fn update_worker_health(
        &self,
        update: &WorkerHealthUpdate,
    ) -> Result<CasOutcome, PersistenceError> {
        let refreshed_at = update.capabilities.refreshed_at.map(timestamp);
        let result = sqlx::query(
            "UPDATE workers SET online = ?, capabilities_json = ?, capabilities_refreshed_at_ms = ?,
                last_seen_at_ms = ?, health_retry_count = ?, next_health_check_at_ms = ?,
                last_error = ?, version = version + 1, updated_at_ms = ?
             WHERE id = ? AND version = ?",
        )
        .bind(update.online)
        .bind(encode_json("capabilities_json", &update.capabilities)?)
        .bind(refreshed_at)
        .bind(update.last_seen_at.map(timestamp))
        .bind(i64::from(update.health_retry_count))
        .bind(update.next_health_check_at.map(timestamp))
        .bind(update.last_error.as_deref())
        .bind(timestamp(update.updated_at))
        .bind(update.id.to_string())
        .bind(sqlite_u64("expected_version", update.expected_version)?)
        .execute(self.database.pool())
        .await?;
        Ok(cas(result.rows_affected(), update.expected_version))
    }

    /// # Errors
    /// Returns an error when `SQLite` access or count conversion fails.
    pub async fn worker_used_slots(&self, id: WorkerId) -> Result<u64, PersistenceError> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM tasks
             WHERE worker_id = ? AND status NOT IN ('completed', 'failed', 'cancelled')",
        )
        .bind(id.to_string())
        .fetch_one(self.database.pool())
        .await?;
        rust_u64("used_slots", count)
    }
}

pub(super) const WORKER_COLUMNS: &str =
    "SELECT id, version, name, api_url, enabled, online, compute_slots,
    capabilities_json, last_seen_at_ms, last_assigned_at_ms, health_retry_count,
    next_health_check_at_ms, created_at_ms, updated_at_ms, last_error
    FROM workers";

pub(super) fn map_worker(row: &sqlx::sqlite::SqliteRow) -> Result<WorkerRecord, PersistenceError> {
    let api_url_value: String = row.try_get("api_url")?;
    let api_url = WorkerApiUrl::parse(&api_url_value)
        .map_err(|_| super::codec::corrupt("api_url", api_url_value))?;
    let slots = rust_u64("compute_slots", row.try_get("compute_slots")?)?;
    Ok(WorkerRecord {
        id: parse_brand::<WorkerId>("id", row.try_get("id")?)?,
        version: rust_u64("version", row.try_get("version")?)?,
        name: WorkerName::new(row.try_get::<String, _>("name")?),
        api_url,
        enabled: boolean("enabled", row.try_get("enabled")?)?,
        online: boolean("online", row.try_get("online")?)?,
        compute_slots: ComputeSlots::try_from(slots)
            .map_err(|_| super::codec::corrupt("compute_slots", slots))?,
        capabilities: decode_json("capabilities_json", row.try_get("capabilities_json")?)?,
        last_seen_at: parse_optional_timestamp("last_seen_at_ms", row.try_get("last_seen_at_ms")?)?,
        last_assigned_at: parse_optional_timestamp(
            "last_assigned_at_ms",
            row.try_get("last_assigned_at_ms")?,
        )?,
        health_retry_count: rust_u32("health_retry_count", row.try_get("health_retry_count")?)?,
        next_health_check_at: parse_optional_timestamp(
            "next_health_check_at_ms",
            row.try_get("next_health_check_at_ms")?,
        )?,
        created_at: parse_timestamp("created_at_ms", row.try_get("created_at_ms")?)?,
        updated_at: parse_timestamp("updated_at_ms", row.try_get("updated_at_ms")?)?,
        last_error: row.try_get("last_error")?,
    })
}

fn cas(rows: u64, expected_version: u64) -> CasOutcome {
    if rows == 1 {
        CasOutcome::Applied {
            new_version: expected_version + 1,
        }
    } else {
        CasOutcome::Conflict
    }
}
