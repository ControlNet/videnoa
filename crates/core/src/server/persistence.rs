use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use tokio_util::sync::CancellationToken;
use tracing::warn;

use super::{CancelReason, CancelRequest, Job, JobStatus, PipelineGraph, ProgressUpdate};

mod idempotency;
#[cfg(test)]
mod tests;
pub(crate) use idempotency::{IdempotentJobClaim, IdempotentJobLookup};

/// Version of the `jobs.db` schema, kept in `PRAGMA user_version`.
///
/// 1. The jobs table with its idempotency columns and index. Databases from
///    before versioning (version 0) reach it through the original idempotent
///    `CREATE TABLE IF NOT EXISTS` and column probing.
/// 2. `cancel_requested_at` and `cancel_reason`.
///
/// A database with a higher version came from a newer build and is refused,
/// so a downgrade cannot rewrite rows it does not understand.
const SCHEMA_VERSION: i64 = 2;

const STATUS_QUEUED: &str = "queued";
const STATUS_RUNNING: &str = "running";
const STATUS_COMPLETED: &str = "completed";
const STATUS_FAILED: &str = "failed";
const STATUS_CANCELLED: &str = "cancelled";

#[derive(Debug)]
struct PersistedJobRow {
    id: String,
    status: JobStatus,
    workflow_json: String,
    created_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
    progress_json: Option<String>,
    error: Option<String>,
    params_json: Option<String>,
    workflow_name: String,
    workflow_source: String,
    rerun_of_job_id: Option<String>,
    cancel_requested_at: Option<DateTime<Utc>>,
    cancel_reason: Option<CancelReason>,
}

#[derive(Debug, Clone)]
pub(crate) struct JobsPersistence {
    db_path: PathBuf,
}

impl JobsPersistence {
    pub(crate) fn new(data_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(data_dir).with_context(|| {
            format!(
                "failed to create data directory for jobs db: {}",
                data_dir.display()
            )
        })?;

        let persistence = Self {
            db_path: data_dir.join("jobs.db"),
        };
        persistence.initialize_schema()?;
        Ok(persistence)
    }

    pub(crate) fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub(crate) fn upsert_job(&self, job: &Job) -> Result<()> {
        let row = Self::row_from_job(job)?;
        self.with_connection(|conn| self.upsert_row(conn, &row))
    }

    /// A late executor snapshot must not resurrect a deleted history row.
    pub(crate) fn update_job(&self, job: &Job) -> Result<()> {
        let row = Self::row_from_job(job)?;
        self.with_connection(|conn| {
            conn.execute(
                "UPDATE jobs SET status = ?2, started_at = ?3, completed_at = ?4,
                 progress_json = ?5, error = ?6, updated_at = ?7,
                 cancel_requested_at = ?8, cancel_reason = ?9 WHERE id = ?1",
                params![
                    row.id,
                    status_to_str(row.status),
                    row.started_at.map(|ts| ts.to_rfc3339()),
                    row.completed_at.map(|ts| ts.to_rfc3339()),
                    row.progress_json,
                    row.error,
                    Utc::now().to_rfc3339(),
                    row.cancel_requested_at.map(|ts| ts.to_rfc3339()),
                    row.cancel_reason.map(cancel_reason_to_str),
                ],
            )?;
            Ok(())
        })
    }

    pub(crate) fn load_jobs_for_startup(&self) -> Result<Vec<Job>> {
        self.with_connection(|conn| {
            let mut stmt = conn.prepare(
                "SELECT
                    id,
                    status,
                    workflow_json,
                    created_at,
                    started_at,
                    completed_at,
                    progress_json,
                    error,
                    params_json,
                    workflow_name,
                    workflow_source,
                    rerun_of_job_id,
                    cancel_requested_at,
                    cancel_reason
                 FROM jobs
                 ORDER BY created_at ASC, id ASC",
            )?;

            // Each row carries its id separately, so a row that cannot be
            // decoded is still reported by id.
            let raw_rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, Self::decode_row(row)))
            })?;

            let startup_now = Utc::now();
            let mut jobs = Vec::new();
            let mut skipped = Vec::new();

            for row_result in raw_rows {
                let (id, decoded) = row_result?;
                let mut row = match decoded {
                    Ok(row) => row,
                    Err(err) => {
                        warn!(job_id = %id, error = %err, "Skipping persisted job row that cannot be decoded");
                        skipped.push(id);
                        continue;
                    }
                };

                if matches!(row.status, JobStatus::Queued | JobStatus::Running) {
                    let previous_status = row.status;
                    row.status = JobStatus::Cancelled;
                    row.completed_at = Some(row.completed_at.unwrap_or(startup_now));
                    row.error = Some(startup_reconciliation_error(previous_status, row.error.as_deref()));
                    // A cancel a user asked for before the stop keeps its reason.
                    if row.cancel_reason.is_none() {
                        row.cancel_requested_at = Some(startup_now);
                        row.cancel_reason = Some(CancelReason::Restart);
                    }

                    self.upsert_row(conn, &row).with_context(|| {
                        format!("failed to reconcile startup status for job {}", row.id)
                    })?;
                }

                let workflow: PipelineGraph = match serde_json::from_str(&row.workflow_json) {
                    Ok(workflow) => workflow,
                    Err(err) => {
                        warn!(job_id = %row.id, error = %err, "Skipping persisted job with invalid workflow snapshot");
                        skipped.push(row.id);
                        continue;
                    }
                };

                let params: Option<HashMap<String, serde_json::Value>> =
                    match row.params_json.as_deref() {
                        Some(encoded) => match serde_json::from_str(encoded) {
                            Ok(parsed) => Some(parsed),
                            Err(err) => {
                                warn!(job_id = %row.id, error = %err, "Skipping persisted job with invalid params snapshot");
                                skipped.push(row.id);
                                continue;
                            }
                        },
                        None => None,
                    };

                let progress: Option<ProgressUpdate> = match row.progress_json.as_deref() {
                    Some(encoded) => match serde_json::from_str(encoded) {
                        Ok(parsed) => Some(parsed),
                        Err(err) => {
                            warn!(job_id = %row.id, error = %err, "Dropping invalid persisted progress snapshot");
                            None
                        }
                    },
                    None => None,
                };

                let cancel_request = row
                    .cancel_reason
                    .zip(row.cancel_requested_at)
                    .map(|(reason, requested_at)| CancelRequest {
                        requested_at,
                        reason,
                    });
                jobs.push(Job {
                    id: row.id,
                    status: row.status,
                    workflow,
                    created_at: row.created_at,
                    started_at: row.started_at,
                    completed_at: row.completed_at,
                    progress,
                    error: row.error,
                    cancel_token: CancellationToken::new(),
                    params,
                    workflow_name: row.workflow_name,
                    workflow_source: row.workflow_source,
                    rerun_of_job_id: row.rerun_of_job_id,
                    cancel_request,
                });
            }

            if !skipped.is_empty() {
                warn!(
                    count = skipped.len(),
                    job_ids = ?skipped,
                    "Persisted jobs that cannot be loaded are not listed; DELETE /api/jobs/{{id}} removes them"
                );
            }

            Ok(jobs)
        })
    }

    /// Decodes one `jobs` row selected by [`Self::load_jobs_for_startup`].
    fn decode_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PersistedJobRow> {
        let status_raw: String = row.get(1)?;
        let status = parse_status(&status_raw).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                1,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("unknown persisted status: {status_raw}"),
                )),
            )
        })?;

        Ok(PersistedJobRow {
            id: row.get(0)?,
            status,
            workflow_json: row.get(2)?,
            created_at: parse_timestamp(row.get::<_, String>(3)?.as_str()).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    3,
                    rusqlite::types::Type::Text,
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        e.to_string(),
                    )),
                )
            })?,
            started_at: parse_optional_timestamp(row.get::<_, Option<String>>(4)?).map_err(
                |e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        4,
                        rusqlite::types::Type::Text,
                        Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            e.to_string(),
                        )),
                    )
                },
            )?,
            completed_at: parse_optional_timestamp(row.get::<_, Option<String>>(5)?).map_err(
                |e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        5,
                        rusqlite::types::Type::Text,
                        Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            e.to_string(),
                        )),
                    )
                },
            )?,
            progress_json: row.get(6)?,
            error: row.get(7)?,
            params_json: row.get(8)?,
            workflow_name: row.get(9)?,
            workflow_source: row.get(10)?,
            rerun_of_job_id: row.get(11)?,
            cancel_requested_at: parse_optional_timestamp(row.get::<_, Option<String>>(12)?)
                .map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        12,
                        rusqlite::types::Type::Text,
                        Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            e.to_string(),
                        )),
                    )
                })?,
            // A reason this build does not know reads as no reason.
            cancel_reason: row
                .get::<_, Option<String>>(13)?
                .as_deref()
                .and_then(parse_cancel_reason),
        })
    }

    pub(crate) fn delete_job(&self, job_id: &str) -> Result<usize> {
        self.with_connection(|conn| {
            let deleted_rows = conn
                .execute("DELETE FROM jobs WHERE id = ?1", params![job_id])
                .with_context(|| format!("failed to delete persisted job {job_id}"))?;
            Ok(deleted_rows)
        })
    }

    fn initialize_schema(&self) -> Result<()> {
        self.with_connection(|conn| {
            conn.execute_batch("PRAGMA journal_mode = WAL;")
                .with_context(|| format!("failed to open jobs db: {}", self.db_path.display()))?;
            let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
            if version > SCHEMA_VERSION {
                bail!(
                    "jobs db {} has schema version {version}, newer than this build supports \
                     ({SCHEMA_VERSION}); run a newer Videnoa or move the file away",
                    self.db_path.display()
                );
            }
            // Migrations run in order. Each one is idempotent and records its
            // version last, so one interrupted by a crash simply runs again.
            if version < 1 {
                self.migrate_to_v1(conn)?;
            }
            if version < 2 {
                Self::migrate_to_v2(conn).with_context(|| {
                    format!(
                        "failed to migrate jobs db to schema version 2: {}",
                        self.db_path.display()
                    )
                })?;
            }
            Ok(())
        })
    }

    fn migrate_to_v1(&self, conn: &mut Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS jobs (
                    id TEXT PRIMARY KEY,
                    status TEXT NOT NULL,
                    workflow_json TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    started_at TEXT,
                    completed_at TEXT,
                    progress_json TEXT,
                    error TEXT,
                    params_json TEXT,
                    workflow_name TEXT NOT NULL,
                    workflow_source TEXT NOT NULL,
                    rerun_of_job_id TEXT,
                    updated_at TEXT NOT NULL,
                    idempotency_key TEXT,
                    request_fingerprint TEXT
                 );
                 CREATE INDEX IF NOT EXISTS idx_jobs_created_at ON jobs(created_at DESC);
                 CREATE INDEX IF NOT EXISTS idx_jobs_status ON jobs(status);",
        )
        .with_context(|| {
            format!(
                "failed to initialize jobs persistence schema: {}",
                self.db_path.display()
            )
        })?;
        Self::migrate_idempotency_schema(conn).map_err(|error| {
            anyhow::anyhow!(
                "failed to migrate jobs idempotency schema at {}: {error:#}",
                self.db_path.display()
            )
        })?;
        conn.execute_batch("PRAGMA user_version = 1;")?;
        Ok(())
    }

    fn migrate_to_v2(conn: &mut Connection) -> Result<()> {
        let transaction =
            conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let columns = table_columns(&transaction)?;
        for column in ["cancel_requested_at", "cancel_reason"] {
            if !columns.iter().any(|existing| existing == column) {
                transaction.execute(&format!("ALTER TABLE jobs ADD COLUMN {column} TEXT"), [])?;
            }
        }
        transaction.execute_batch("PRAGMA user_version = 2;")?;
        transaction.commit()?;
        Ok(())
    }

    fn with_connection<T>(&self, op: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T> {
        let mut conn = Connection::open(&self.db_path)
            .with_context(|| format!("failed to open jobs db: {}", self.db_path.display()))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .context("failed to set jobs db busy timeout")?;
        op(&mut conn)
    }

    fn upsert_row(&self, conn: &Connection, row: &PersistedJobRow) -> Result<()> {
        let updated_at = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO jobs (
                id,
                status,
                workflow_json,
                created_at,
                started_at,
                completed_at,
                progress_json,
                error,
                params_json,
                workflow_name,
                workflow_source,
                rerun_of_job_id,
                updated_at,
                cancel_requested_at,
                cancel_reason
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
             ON CONFLICT(id) DO UPDATE SET
                status = excluded.status,
                workflow_json = excluded.workflow_json,
                created_at = excluded.created_at,
                started_at = excluded.started_at,
                completed_at = excluded.completed_at,
                progress_json = excluded.progress_json,
                error = excluded.error,
                params_json = excluded.params_json,
                workflow_name = excluded.workflow_name,
                workflow_source = excluded.workflow_source,
                rerun_of_job_id = excluded.rerun_of_job_id,
                updated_at = excluded.updated_at,
                cancel_requested_at = excluded.cancel_requested_at,
                cancel_reason = excluded.cancel_reason",
            params![
                row.id,
                status_to_str(row.status),
                row.workflow_json,
                row.created_at.to_rfc3339(),
                row.started_at.map(|ts| ts.to_rfc3339()),
                row.completed_at.map(|ts| ts.to_rfc3339()),
                row.progress_json,
                row.error,
                row.params_json,
                row.workflow_name,
                row.workflow_source,
                row.rerun_of_job_id,
                updated_at,
                row.cancel_requested_at.map(|ts| ts.to_rfc3339()),
                row.cancel_reason.map(cancel_reason_to_str),
            ],
        )
        .with_context(|| format!("failed to upsert persisted job {}", row.id))?;

        Ok(())
    }

    fn row_from_job(job: &Job) -> Result<PersistedJobRow> {
        Ok(PersistedJobRow {
            id: job.id.clone(),
            status: job.status,
            workflow_json: serde_json::to_string(&job.workflow)
                .context("failed to serialize workflow snapshot")?,
            created_at: job.created_at,
            started_at: job.started_at,
            completed_at: job.completed_at,
            progress_json: encode_optional_json(job.progress.as_ref())
                .context("failed to serialize progress snapshot")?,
            error: job.error.clone(),
            params_json: encode_optional_json(job.params.as_ref())
                .context("failed to serialize params snapshot")?,
            workflow_name: job.workflow_name.clone(),
            workflow_source: job.workflow_source.clone(),
            rerun_of_job_id: job.rerun_of_job_id.clone(),
            cancel_requested_at: job.cancel_request.map(|request| request.requested_at),
            cancel_reason: job.cancel_request.map(|request| request.reason),
        })
    }
}

fn table_columns(connection: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut statement = connection.prepare("PRAGMA table_info(jobs)")?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect();
    columns
}

fn cancel_reason_to_str(reason: CancelReason) -> &'static str {
    match reason {
        CancelReason::User => "user",
        CancelReason::Shutdown => "shutdown",
        CancelReason::Restart => "restart",
    }
}

fn parse_cancel_reason(value: &str) -> Option<CancelReason> {
    match value {
        "user" => Some(CancelReason::User),
        "shutdown" => Some(CancelReason::Shutdown),
        "restart" => Some(CancelReason::Restart),
        _ => None,
    }
}

fn encode_optional_json<T: serde::Serialize>(value: Option<&T>) -> Result<Option<String>> {
    match value {
        Some(value) => Ok(Some(serde_json::to_string(value)?)),
        None => Ok(None),
    }
}

fn parse_timestamp(value: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .with_context(|| format!("invalid RFC3339 timestamp: {value}"))
        .map(|ts| ts.with_timezone(&Utc))
}

fn parse_optional_timestamp(value: Option<String>) -> Result<Option<DateTime<Utc>>> {
    value
        .as_deref()
        .map(parse_timestamp)
        .transpose()
        .with_context(|| "invalid optional RFC3339 timestamp".to_string())
}

fn status_to_str(status: JobStatus) -> &'static str {
    match status {
        JobStatus::Queued => STATUS_QUEUED,
        JobStatus::Running => STATUS_RUNNING,
        JobStatus::Completed => STATUS_COMPLETED,
        JobStatus::Failed => STATUS_FAILED,
        JobStatus::Cancelled => STATUS_CANCELLED,
    }
}

fn parse_status(value: &str) -> Option<JobStatus> {
    match value {
        STATUS_QUEUED => Some(JobStatus::Queued),
        STATUS_RUNNING => Some(JobStatus::Running),
        STATUS_COMPLETED => Some(JobStatus::Completed),
        STATUS_FAILED => Some(JobStatus::Failed),
        STATUS_CANCELLED => Some(JobStatus::Cancelled),
        _ => None,
    }
}

fn startup_reconciliation_error(
    previous_status: JobStatus,
    existing_error: Option<&str>,
) -> String {
    let base = format!(
        "job restored from persisted '{status}' state at startup and transitioned to 'cancelled' for retry safety",
        status = status_to_str(previous_status)
    );

    match existing_error {
        Some(existing) if !existing.trim().is_empty() => {
            format!("{base}; previous_error={existing}")
        }
        _ => base,
    }
}
