use std::net::IpAddr;

use sqlx::Row;

use crate::domain::{
    AuthSettingsDto, ComputeSlots, ConcurrencyLimit, RetrySettingsDto, SchedulerStatus,
    ServerSettingsDto, TimeoutSettingsDto,
};

use super::codec::{boolean, parse_timestamp, rust_u32, rust_u64, sqlite_u64, timestamp};
use super::models::{CasOutcome, ConfigurationUpdate, SettingsRecord, SettingsUpdate};
use super::{PersistenceError, Store};

const SETTINGS_COLUMNS: &str = "version, server_host, server_port, secure_cookie,
    session_absolute_seconds, session_idle_seconds, paused, default_compute_slots,
    prefetch_per_worker, max_concurrent_uploads, max_concurrent_downloads,
    health_seconds, poll_seconds, transfer_seconds, retry_initial_seconds,
    retry_maximum_seconds, retry_max_attempts, updated_at_ms, config_document,
    pending_config_document, configuration_initialized";

impl Store {
    /// Loads the authoritative controller settings record.
    ///
    /// # Errors
    /// Returns an error when the database row cannot be loaded or decoded.
    pub async fn settings(&self) -> Result<SettingsRecord, PersistenceError> {
        let query = format!("SELECT {SETTINGS_COLUMNS} FROM controller_settings WHERE id = 1");
        let row = sqlx::query(&query).fetch_one(self.database.pool()).await?;
        decode_settings(&row)
    }

    /// Seeds durable settings when configuration has not yet been initialized.
    ///
    /// # Errors
    /// Returns an error when values cannot be represented by `SQLite` or the database update fails.
    pub async fn initialize_settings(
        &self,
        update: &ConfigurationUpdate,
    ) -> Result<SettingsRecord, PersistenceError> {
        let result = configuration_query(
            "UPDATE controller_settings SET server_host = ?, server_port = ?, secure_cookie = ?,
             session_absolute_seconds = ?, session_idle_seconds = ?, paused = ?,
             default_compute_slots = ?, prefetch_per_worker = ?, max_concurrent_uploads = ?,
             max_concurrent_downloads = ?, health_seconds = ?, poll_seconds = ?,
             transfer_seconds = ?, retry_initial_seconds = ?, retry_maximum_seconds = ?,
             retry_max_attempts = ?, updated_at_ms = ?, config_document = ?,
             pending_config_document = NULL, configuration_initialized = 1
             WHERE id = 1 AND configuration_initialized = 0",
            update,
        )?
        .execute(self.database.pool())
        .await?;
        if result.rows_affected() == 0 {
            return self.settings().await;
        }
        self.settings().await
    }

    /// Imports an offline configuration edit into the authoritative settings record.
    ///
    /// # Errors
    /// Returns an error when values cannot be represented by `SQLite` or the database update fails.
    pub async fn import_settings(
        &self,
        update: &ConfigurationUpdate,
    ) -> Result<SettingsRecord, PersistenceError> {
        configuration_query(
            "UPDATE controller_settings SET server_host = ?, server_port = ?, secure_cookie = ?,
             session_absolute_seconds = ?, session_idle_seconds = ?, paused = ?,
             default_compute_slots = ?, prefetch_per_worker = ?, max_concurrent_uploads = ?,
             max_concurrent_downloads = ?, health_seconds = ?, poll_seconds = ?,
             transfer_seconds = ?, retry_initial_seconds = ?, retry_maximum_seconds = ?,
             retry_max_attempts = ?, updated_at_ms = ?, config_document = ?,
             pending_config_document = NULL, configuration_initialized = 1, version = version + 1
             WHERE id = 1",
            update,
        )?
        .execute(self.database.pool())
        .await?;
        self.settings().await
    }

    /// Applies a complete configuration update using compare-and-swap versioning.
    ///
    /// # Errors
    /// Returns an error when values cannot be represented by `SQLite` or the database update fails.
    pub async fn update_configuration(
        &self,
        update: &ConfigurationUpdate,
    ) -> Result<CasOutcome, PersistenceError> {
        let result = configuration_query(
            "UPDATE controller_settings SET server_host = ?, server_port = ?, secure_cookie = ?,
             session_absolute_seconds = ?, session_idle_seconds = ?, paused = ?,
             default_compute_slots = ?, prefetch_per_worker = ?, max_concurrent_uploads = ?,
             max_concurrent_downloads = ?, health_seconds = ?, poll_seconds = ?,
             transfer_seconds = ?, retry_initial_seconds = ?, retry_maximum_seconds = ?,
             retry_max_attempts = ?, updated_at_ms = ?, config_document = ?,
             pending_config_document = CASE WHEN config_document = ? THEN NULL ELSE ? END,
             configuration_initialized = 1, version = version + 1
             WHERE id = 1 AND version = ?",
            update,
        )?
        .bind(&update.config_document)
        .bind(&update.config_document)
        .bind(sqlite_u64("expected_version", update.expected_version)?)
        .execute(self.database.pool())
        .await?;
        match result.rows_affected() {
            1 => Ok(CasOutcome::Applied {
                new_version: update.expected_version + 1,
            }),
            _ => Ok(CasOutcome::Conflict),
        }
    }

    /// Applies scheduler-only settings using compare-and-swap versioning.
    ///
    /// # Errors
    /// Returns an error when values cannot be represented by `SQLite` or the database update fails.
    pub async fn update_settings(
        &self,
        update: &SettingsUpdate,
    ) -> Result<CasOutcome, PersistenceError> {
        let result = sqlx::query(
            "UPDATE controller_settings SET paused = ?, default_compute_slots = ?,
             prefetch_per_worker = ?, max_concurrent_uploads = ?, max_concurrent_downloads = ?,
             health_seconds = ?, poll_seconds = ?, transfer_seconds = ?, retry_initial_seconds = ?,
             retry_maximum_seconds = ?, retry_max_attempts = ?, updated_at_ms = ?, version = version + 1
             WHERE id = 1 AND version = ?",
        )
        .bind(update.scheduler.paused)
        .bind(i64::from(update.scheduler.default_compute_slots.get()))
        .bind(i64::from(update.scheduler.prefetch_per_worker))
        .bind(i64::from(update.scheduler.max_concurrent_uploads.get()))
        .bind(i64::from(update.scheduler.max_concurrent_downloads.get()))
        .bind(sqlite_u64("health_seconds", update.timeouts.health_seconds)?)
        .bind(sqlite_u64("poll_seconds", update.timeouts.poll_seconds)?)
        .bind(sqlite_u64("transfer_seconds", update.timeouts.transfer_seconds)?)
        .bind(sqlite_u64("retry_initial_seconds", update.retry.initial_seconds)?)
        .bind(sqlite_u64("retry_maximum_seconds", update.retry.maximum_seconds)?)
        .bind(i64::from(update.retry.max_attempts))
        .bind(timestamp(update.updated_at))
        .bind(sqlite_u64("expected_version", update.expected_version)?)
        .execute(self.database.pool())
        .await?;
        match result.rows_affected() {
            1 => Ok(CasOutcome::Applied {
                new_version: update.expected_version + 1,
            }),
            _ => Ok(CasOutcome::Conflict),
        }
    }

    /// Clears a pending configuration projection for the matching durable version.
    ///
    /// # Errors
    /// Returns an error when the version cannot be represented by `SQLite` or the update fails.
    pub async fn complete_config_projection(&self, version: u64) -> Result<bool, PersistenceError> {
        let result = sqlx::query(
            "UPDATE controller_settings SET pending_config_document = NULL
             WHERE id = 1 AND version = ? AND pending_config_document IS NOT NULL",
        )
        .bind(sqlite_u64("version", version)?)
        .execute(self.database.pool())
        .await?;
        Ok(result.rows_affected() == 1)
    }
}

fn configuration_query<'a>(
    sql: &'a str,
    update: &'a ConfigurationUpdate,
) -> Result<sqlx::query::Query<'a, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'a>>, PersistenceError>
{
    Ok(sqlx::query(sql)
        .bind(update.server.host.to_string())
        .bind(i64::from(update.server.port))
        .bind(update.auth.secure_cookie)
        .bind(sqlite_duration(update.auth.session_absolute_seconds)?)
        .bind(sqlite_duration(update.auth.session_idle_seconds)?)
        .bind(update.scheduler.paused)
        .bind(i64::from(update.scheduler.default_compute_slots.get()))
        .bind(i64::from(update.scheduler.prefetch_per_worker))
        .bind(i64::from(update.scheduler.max_concurrent_uploads.get()))
        .bind(i64::from(update.scheduler.max_concurrent_downloads.get()))
        .bind(sqlite_duration(update.timeouts.health_seconds)?)
        .bind(sqlite_duration(update.timeouts.poll_seconds)?)
        .bind(sqlite_duration(update.timeouts.transfer_seconds)?)
        .bind(sqlite_duration(update.retry.initial_seconds)?)
        .bind(sqlite_duration(update.retry.maximum_seconds)?)
        .bind(i64::from(update.retry.max_attempts))
        .bind(timestamp(update.updated_at))
        .bind(&update.config_document))
}

fn sqlite_duration(value: u64) -> Result<i64, PersistenceError> {
    sqlite_u64("duration_seconds", value)
}

fn decode_settings(row: &sqlx::sqlite::SqliteRow) -> Result<SettingsRecord, PersistenceError> {
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
    let host = row
        .try_get::<String, _>("server_host")?
        .parse::<IpAddr>()
        .map_err(|_| super::codec::corrupt("server_host", "invalid IP address"))?;
    Ok(SettingsRecord {
        version: rust_u64("version", row.try_get("version")?)?,
        server: ServerSettingsDto {
            host,
            port: u16::try_from(rust_u64("server_port", row.try_get("server_port")?)?)
                .map_err(|_| super::codec::corrupt("server_port", "overflow"))?,
        },
        auth: AuthSettingsDto {
            secure_cookie: boolean("secure_cookie", row.try_get("secure_cookie")?)?,
            session_absolute_seconds: rust_u64(
                "session_absolute_seconds",
                row.try_get("session_absolute_seconds")?,
            )?,
            session_idle_seconds: rust_u64(
                "session_idle_seconds",
                row.try_get("session_idle_seconds")?,
            )?,
        },
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
        config_document: row.try_get("config_document")?,
        pending_config_document: row.try_get("pending_config_document")?,
        configuration_initialized: boolean(
            "configuration_initialized",
            row.try_get("configuration_initialized")?,
        )?,
    })
}
