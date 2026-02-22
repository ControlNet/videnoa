use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use tokio_util::sync::CancellationToken;
use tracing::warn;

use super::{Job, JobStatus, PipelineGraph, ProgressUpdate};

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
                    rerun_of_job_id
                 FROM jobs
                 ORDER BY created_at ASC, id ASC",
            )?;

            let raw_rows = stmt.query_map([], |row| {
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
                })
            })?;

            let startup_now = Utc::now();
            let mut jobs = Vec::new();

            for row_result in raw_rows {
                let mut row = match row_result {
                    Ok(row) => row,
                    Err(err) => {
                        warn!(error = %err, "Skipping invalid persisted job row");
                        continue;
                    }
                };

                if matches!(row.status, JobStatus::Queued | JobStatus::Running) {
                    let previous_status = row.status;
                    row.status = JobStatus::Cancelled;
                    row.completed_at = Some(row.completed_at.unwrap_or(startup_now));
                    row.error = Some(startup_reconciliation_error(previous_status, row.error.as_deref()));

                    self.upsert_row(conn, &row).with_context(|| {
                        format!("failed to reconcile startup status for job {}", row.id)
                    })?;
                }

                let workflow: PipelineGraph = match serde_json::from_str(&row.workflow_json) {
                    Ok(workflow) => workflow,
                    Err(err) => {
                        warn!(job_id = %row.id, error = %err, "Skipping persisted job with invalid workflow snapshot");
                        continue;
                    }
                };

                let params: Option<HashMap<String, serde_json::Value>> =
                    match row.params_json.as_deref() {
                        Some(encoded) => match serde_json::from_str(encoded) {
                            Ok(parsed) => Some(parsed),
                            Err(err) => {
                                warn!(job_id = %row.id, error = %err, "Skipping persisted job with invalid params snapshot");
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
                });
            }

            Ok(jobs)
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
            conn.execute_batch(
                "PRAGMA journal_mode = WAL;
                 CREATE TABLE IF NOT EXISTS jobs (
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
                    updated_at TEXT NOT NULL
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
            Ok(())
        })
    }

    fn with_connection<T>(&self, op: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let conn = Connection::open(&self.db_path)
            .with_context(|| format!("failed to open jobs db: {}", self.db_path.display()))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .context("failed to set jobs db busy timeout")?;
        op(&conn)
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
                updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
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
                updated_at = excluded.updated_at",
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
        })
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
