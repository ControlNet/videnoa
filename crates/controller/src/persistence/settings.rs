use sqlx::Row;

use crate::domain::{
    ComputeSlots, ConcurrencyLimit, RetrySettingsDto, SchedulerStatus, TimeoutSettingsDto,
};

use super::codec::{boolean, parse_timestamp, rust_u32, rust_u64, sqlite_u64, timestamp};
use super::models::{CasOutcome, SettingsRecord, SettingsUpdate};
use super::{PersistenceError, Store};

impl Store {
    /// # Errors
    /// Returns an error when `SQLite` access or persisted decoding fails.
    pub async fn settings(&self) -> Result<SettingsRecord, PersistenceError> {
        let row = sqlx::query(
            "SELECT version, paused, default_compute_slots, prefetch_per_worker,
                max_concurrent_uploads, max_concurrent_downloads, health_seconds,
                poll_seconds, transfer_seconds, retry_initial_seconds,
                retry_maximum_seconds, retry_max_attempts, updated_at_ms
             FROM controller_settings WHERE id = 1",
        )
        .fetch_one(self.database.pool())
        .await?;
        let slots = rust_u64(
            "default_compute_slots",
            row.try_get("default_compute_slots")?,
        )?;
        let uploads = rust_u64(
            "max_concurrent_uploads",
            row.try_get("max_concurrent_uploads")?,
        )?;
        let downloads = rust_u64(
            "max_concurrent_downloads",
            row.try_get("max_concurrent_downloads")?,
        )?;
        Ok(SettingsRecord {
            version: rust_u64("version", row.try_get("version")?)?,
            scheduler: SchedulerStatus {
                paused: boolean("paused", row.try_get("paused")?)?,
                default_compute_slots: ComputeSlots::try_from(slots)
                    .map_err(|_| super::codec::corrupt("default_compute_slots", slots))?,
                prefetch_per_worker: u16::try_from(rust_u64(
                    "prefetch_per_worker",
                    row.try_get("prefetch_per_worker")?,
                )?)
                .map_err(|_| super::codec::corrupt("prefetch_per_worker", "overflow"))?,
                max_concurrent_uploads: ConcurrencyLimit::try_from(uploads)
                    .map_err(|_| super::codec::corrupt("max_concurrent_uploads", uploads))?,
                max_concurrent_downloads: ConcurrencyLimit::try_from(downloads)
                    .map_err(|_| super::codec::corrupt("max_concurrent_downloads", downloads))?,
            },
            timeouts: TimeoutSettingsDto {
                health_seconds: rust_u64("health_seconds", row.try_get("health_seconds")?)?,
                poll_seconds: rust_u64("poll_seconds", row.try_get("poll_seconds")?)?,
                transfer_seconds: rust_u64("transfer_seconds", row.try_get("transfer_seconds")?)?,
            },
            retry: RetrySettingsDto {
                initial_seconds: rust_u64(
                    "retry_initial_seconds",
                    row.try_get("retry_initial_seconds")?,
                )?,
                maximum_seconds: rust_u64(
                    "retry_maximum_seconds",
                    row.try_get("retry_maximum_seconds")?,
                )?,
                max_attempts: rust_u32("retry_max_attempts", row.try_get("retry_max_attempts")?)?,
            },
            updated_at: parse_timestamp("updated_at_ms", row.try_get("updated_at_ms")?)?,
        })
    }

    /// # Errors
    /// Returns an error when `SQLite` access or value encoding fails.
    pub async fn update_settings(
        &self,
        update: &SettingsUpdate,
    ) -> Result<CasOutcome, PersistenceError> {
        let scheduler = &update.scheduler;
        let result = sqlx::query(
            "UPDATE controller_settings SET paused = ?, default_compute_slots = ?,
                prefetch_per_worker = ?, max_concurrent_uploads = ?,
                max_concurrent_downloads = ?, health_seconds = ?, poll_seconds = ?,
                transfer_seconds = ?, retry_initial_seconds = ?, retry_maximum_seconds = ?,
                retry_max_attempts = ?, updated_at_ms = ?, version = version + 1
             WHERE id = 1 AND version = ?",
        )
        .bind(scheduler.paused)
        .bind(i64::from(scheduler.default_compute_slots.get()))
        .bind(i64::from(scheduler.prefetch_per_worker))
        .bind(i64::from(scheduler.max_concurrent_uploads.get()))
        .bind(i64::from(scheduler.max_concurrent_downloads.get()))
        .bind(sqlite_u64(
            "health_seconds",
            update.timeouts.health_seconds,
        )?)
        .bind(sqlite_u64("poll_seconds", update.timeouts.poll_seconds)?)
        .bind(sqlite_u64(
            "transfer_seconds",
            update.timeouts.transfer_seconds,
        )?)
        .bind(sqlite_u64(
            "retry_initial_seconds",
            update.retry.initial_seconds,
        )?)
        .bind(sqlite_u64(
            "retry_maximum_seconds",
            update.retry.maximum_seconds,
        )?)
        .bind(i64::from(update.retry.max_attempts))
        .bind(timestamp(update.updated_at))
        .bind(sqlite_u64("expected_version", update.expected_version)?)
        .execute(self.database.pool())
        .await?;
        if result.rows_affected() == 1 {
            Ok(CasOutcome::Applied {
                new_version: update.expected_version + 1,
            })
        } else {
            Ok(CasOutcome::Conflict)
        }
    }
}
