use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, TransactionBehavior};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

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
/// 3. `job_staged_files` tracking runtime output and staged file paths for
///    interrupted-job cleanup.
/// 4. `dir_dev` and `dir_ino` on `job_staged_files` so startup cleanup can
///    verify the target volume/directory identity before clearing records.
///
/// A database with a higher version came from a newer build and is refused,
/// so a downgrade cannot rewrite rows it does not understand.
const SCHEMA_VERSION: i64 = 4;
const PENDING_TERMINAL_DIR: &str = "pending_terminal";

/// Identifies the filesystem volume and directory inode where a staged output
/// was created, allowing startup cleanup to distinguish a genuinely cleaned
/// directory from an unmounted mountpoint's empty underlying directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StagedDirIdentity {
    pub dev: u64,
    pub ino: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecordedStagedFile {
    pub job_id: String,
    pub path: PathBuf,
    pub dir_identity: Option<StagedDirIdentity>,
}

/// Durable fallback record written when a finished job's SQLite terminal
/// update fails, replayed on startup before interrupted running jobs are
/// reconciled to `cancelled`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PendingTerminalTransition {
    pub job_id: String,
    pub status: JobStatus,
    pub completed_at: DateTime<Utc>,
    pub error: Option<String>,
    pub cancel_requested_at: Option<DateTime<Utc>>,
    pub cancel_reason: Option<CancelReason>,
}

pub(super) const STATUS_ADMITTING: &str = "admitting";
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
    pending_terminal_dir: PathBuf,
    /// One connection for the life of the store. A connection per operation
    /// made every write also checkpoint and delete the WAL when it closed,
    /// several extra fsyncs and file creations per job transition.
    connection: Arc<Mutex<Connection>>,
}

impl JobsPersistence {
    pub(crate) fn new(data_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(data_dir).with_context(|| {
            format!(
                "failed to create data directory for jobs db: {}",
                data_dir.display()
            )
        })?;

        let db_path = data_dir.join("jobs.db");
        let pending_terminal_dir = data_dir.join(PENDING_TERMINAL_DIR);
        let connection = Connection::open(&db_path)
            .with_context(|| format!("failed to open jobs db: {}", db_path.display()))?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .context("failed to set jobs db busy timeout")?;
        let persistence = Self {
            db_path,
            pending_terminal_dir,
            connection: Arc::new(Mutex::new(connection)),
        };
        persistence.initialize_schema()?;
        Ok(persistence)
    }

    pub(crate) fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub(crate) fn pending_terminal_sidecar_path(&self, job_id: &str) -> PathBuf {
        self.pending_terminal_dir.join(format!("{job_id}.json"))
    }

    /// Writes an atomic, synced fallback record for a terminal transition that
    /// could not be committed to `jobs.db`, so a crash before retry succeeds
    /// still recovers the terminal outcome on the next start.
    pub(crate) fn write_pending_terminal_sidecar(
        &self,
        pending: &PendingTerminalTransition,
    ) -> Result<()> {
        std::fs::create_dir_all(&self.pending_terminal_dir).with_context(|| {
            format!(
                "failed to create pending terminal directory: {}",
                self.pending_terminal_dir.display()
            )
        })?;
        let bytes =
            serde_json::to_vec(pending).context("failed to serialize pending terminal record")?;
        let prefix = format!(".{}.", pending.job_id);
        let mut builder = tempfile::Builder::new();
        builder.prefix(&prefix).suffix(".videnoa-pending");
        #[cfg(unix)]
        builder.permissions(std::os::unix::fs::PermissionsExt::from_mode(0o666));
        let mut staged = builder
            .tempfile_in(&self.pending_terminal_dir)
            .with_context(|| {
                format!(
                    "failed to stage pending terminal record in {}",
                    self.pending_terminal_dir.display()
                )
            })?;
        staged
            .write_all(&bytes)
            .context("failed to write pending terminal record")?;
        staged
            .as_file()
            .sync_all()
            .context("failed to sync pending terminal record")?;
        let target = self.pending_terminal_sidecar_path(&pending.job_id);
        staged
            .persist(&target)
            .map_err(|err| err.error)
            .with_context(|| {
                format!(
                    "failed to persist pending terminal record {}",
                    target.display()
                )
            })?;
        #[cfg(unix)]
        if let Ok(dir) = std::fs::File::open(&self.pending_terminal_dir) {
            let _ = dir.sync_all();
        }
        Ok(())
    }

    pub(crate) fn remove_pending_terminal_sidecar(&self, job_id: &str) {
        let path = self.pending_terminal_sidecar_path(job_id);
        if let Err(error) = std::fs::remove_file(&path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                warn!(
                    job_id = %job_id,
                    path = %path.display(),
                    %error,
                    "Failed to remove pending terminal sidecar"
                );
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn upsert_job(&self, job: &Job) -> Result<()> {
        let row = Self::row_from_job(job)?;
        self.with_connection(|conn| self.upsert_row(conn, &row))
    }

    /// Inserts a newly admitted job in the pre-acceptance `admitting` state
    /// until [`Self::activate_admitted_jobs`] promotes it alongside runtime
    /// publication.
    pub(crate) fn insert_admitting_job(&self, job: &Job) -> Result<()> {
        let row = Self::row_from_job(job)?;
        let updated_at = Utc::now().to_rfc3339();
        self.with_connection(|conn| {
            conn.execute(
                "INSERT INTO jobs (
                    id, status, workflow_json, created_at, started_at, completed_at,
                    progress_json, error, params_json, workflow_name, workflow_source,
                    rerun_of_job_id, updated_at, cancel_requested_at, cancel_reason
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                params![
                    row.id,
                    STATUS_ADMITTING,
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
            .with_context(|| format!("failed to insert admitting job {}", row.id))?;
            Ok(())
        })
    }

    /// Atomically promotes `admitting` job rows to `queued` right as they are
    /// accepted into runtime state.
    pub(crate) fn activate_admitted_jobs(&self, job_ids: &[String]) -> Result<()> {
        if job_ids.is_empty() {
            return Ok(());
        }
        self.with_connection(|conn| {
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let now = Utc::now().to_rfc3339();
            for job_id in job_ids {
                let updated = tx
                    .execute(
                        "UPDATE jobs SET status = ?2, updated_at = ?3
                         WHERE id = ?1 AND status = ?4",
                        params![job_id, STATUS_QUEUED, now, STATUS_ADMITTING],
                    )
                    .with_context(|| format!("failed to activate admitted job {job_id}"))?;
                if updated != 1 {
                    bail!("admitted job {job_id} disappeared before activation");
                }
            }
            tx.commit()?;
            Ok(())
        })
    }

    /// A late executor snapshot must not resurrect a deleted history row.
    pub(crate) fn update_job(&self, job: &Job) -> Result<()> {
        let row = Self::row_from_job(job)?;
        self.with_connection(|conn| {
            let now = Utc::now().to_rfc3339();
            if row.status.is_terminal() {
                let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
                tx.execute(
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
                        now,
                        row.cancel_requested_at.map(|ts| ts.to_rfc3339()),
                        row.cancel_reason.map(cancel_reason_to_str),
                    ],
                )?;
                tx.execute(
                    "DELETE FROM job_staged_files WHERE job_id = ?1",
                    params![row.id],
                )?;
                tx.commit()?;
            } else {
                conn.execute(
                    "UPDATE jobs SET status = ?2, started_at = ?3, completed_at = ?4,
                     progress_json = ?5, error = ?6, updated_at = ?7,
                     cancel_requested_at = COALESCE(cancel_requested_at, ?8),
                     cancel_reason = COALESCE(cancel_reason, ?9)
                     WHERE id = ?1 AND status IN ('queued', 'running')",
                    params![
                        row.id,
                        status_to_str(row.status),
                        row.started_at.map(|ts| ts.to_rfc3339()),
                        row.completed_at.map(|ts| ts.to_rfc3339()),
                        row.progress_json,
                        row.error,
                        now,
                        row.cancel_requested_at.map(|ts| ts.to_rfc3339()),
                        row.cancel_reason.map(cancel_reason_to_str),
                    ],
                )?;
            }
            Ok(())
        })
    }

    pub(crate) fn record_staged_file(&self, job_id: &str, path: &Path) -> Result<()> {
        if path.as_os_str().is_empty() {
            return Ok(());
        }
        let resolved = if path.is_relative() {
            std::env::current_dir()
                .map(|cwd| cwd.join(path))
                .unwrap_or_else(|_| path.to_path_buf())
        } else {
            path.to_path_buf()
        };
        let dir_identity = capture_parent_dir_identity(&resolved);
        let dir_dev = dir_identity.map(|id| id.dev as i64);
        let dir_ino = dir_identity.map(|id| id.ino as i64);
        let encoded = resolved.to_string_lossy();
        self.with_connection(|conn| {
            conn.execute(
                "INSERT INTO job_staged_files (job_id, path, dir_dev, dir_ino)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(job_id, path) DO UPDATE SET
                     dir_dev = COALESCE(excluded.dir_dev, job_staged_files.dir_dev),
                     dir_ino = COALESCE(excluded.dir_ino, job_staged_files.dir_ino)",
                params![job_id, encoded.as_ref(), dir_dev, dir_ino],
            )
            .with_context(|| {
                format!(
                    "failed to record staged output path {} for job {job_id}",
                    resolved.display()
                )
            })?;
            Ok(())
        })
    }

    pub(crate) fn list_staged_file_records(&self) -> Result<Vec<RecordedStagedFile>> {
        self.with_connection(|conn| {
            let mut stmt = conn.prepare(
                "SELECT job_id, path, dir_dev, dir_ino
                 FROM job_staged_files
                 ORDER BY job_id ASC, path ASC",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    let job_id: String = row.get(0)?;
                    let path: String = row.get(1)?;
                    let dir_dev: Option<i64> = row.get(2)?;
                    let dir_ino: Option<i64> = row.get(3)?;
                    let dir_identity = dir_dev.zip(dir_ino).map(|(dev, ino)| StagedDirIdentity {
                        dev: dev as u64,
                        ino: ino as u64,
                    });
                    Ok(RecordedStagedFile {
                        job_id,
                        path: PathBuf::from(path),
                        dir_identity,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    #[cfg(test)]
    pub(crate) fn list_staged_files(&self) -> Result<Vec<(String, PathBuf)>> {
        Ok(self
            .list_staged_file_records()?
            .into_iter()
            .map(|record| (record.job_id, record.path))
            .collect())
    }

    pub(crate) fn remove_staged_file_record(&self, job_id: &str, path: &Path) -> Result<()> {
        let encoded = path.to_string_lossy();
        self.with_connection(|conn| {
            conn.execute(
                "DELETE FROM job_staged_files WHERE job_id = ?1 AND path = ?2",
                params![job_id, encoded.as_ref()],
            )?;
            Ok(())
        })
    }

    /// Records a cancel request on an active (`queued` or `running`) job row
    /// without touching its status or overwriting an earlier cancel reason.
    ///
    /// Returns `true` when an active uncancelled row was updated, or `false`
    /// when the row was already terminal, already carried a cancel request, or
    /// did not exist.
    pub(crate) fn record_cancel_request(
        &self,
        job_id: &str,
        request: &CancelRequest,
    ) -> Result<bool> {
        self.with_connection(|conn| {
            let updated = conn.execute(
                "UPDATE jobs SET cancel_requested_at = ?2, cancel_reason = ?3, updated_at = ?4
                 WHERE id = ?1
                   AND status IN ('queued', 'running')
                   AND cancel_requested_at IS NULL",
                params![
                    job_id,
                    request.requested_at.to_rfc3339(),
                    cancel_reason_to_str(request.reason),
                    Utc::now().to_rfc3339(),
                ],
            )?;
            Ok(updated > 0)
        })
    }

    /// Holds the store's connection, so a test can stall every write.
    #[cfg(test)]
    pub(crate) fn lock_connection(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.connection
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[cfg(test)]
    pub(crate) fn load_jobs_for_startup(&self) -> Result<Vec<Job>> {
        self.load_jobs_for_startup_with_interrupted()
            .map(|(jobs, _)| jobs)
    }

    /// Loads every job, marking those left queued or running by the previous
    /// instance as cancelled, and returns them with the ids of the jobs that
    /// were `running`. Their executors may have left staged output files
    /// behind.
    pub(crate) fn load_jobs_for_startup_with_interrupted(&self) -> Result<(Vec<Job>, Vec<String>)> {
        self.with_connection(|conn| {
            // Drop any pre-acceptance `admitting` rows whose creator process crashed
            // before publishing a receipt or spawning an executor.
            conn.execute(
                "DELETE FROM jobs WHERE status = ?1",
                params![STATUS_ADMITTING],
            )
            .context("failed to purge uncommitted admitting jobs at startup")?;

            let replayed = self.replay_pending_terminal_sidecars(conn)?;
            if replayed > 0 {
                info!(
                    replayed,
                    dir = %self.pending_terminal_dir.display(),
                    "Replayed pending terminal job transitions from sidecar records"
                );
            }

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
            let mut interrupted = Vec::new();

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
                    if previous_status == JobStatus::Running {
                        interrupted.push(row.id.clone());
                    }
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

            Ok((jobs, interrupted))
        })
    }

    /// Decodes one `jobs` row selected by
    /// [`Self::load_jobs_for_startup_with_interrupted`].
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

    fn replay_pending_terminal_sidecars(&self, conn: &mut Connection) -> Result<usize> {
        if !self.pending_terminal_dir.exists() {
            return Ok(0);
        }
        let entries = std::fs::read_dir(&self.pending_terminal_dir).with_context(|| {
            format!(
                "failed to read pending terminal directory {}",
                self.pending_terminal_dir.display()
            )
        })?;
        let mut sidecar_paths = Vec::new();
        for entry in entries {
            let entry = entry.with_context(|| {
                format!(
                    "failed to inspect entry in {}",
                    self.pending_terminal_dir.display()
                )
            })?;
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if name.starts_with('.') {
                if name.ends_with(".videnoa-pending") {
                    let _ = std::fs::remove_file(&path);
                }
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
                sidecar_paths.push(path);
            }
        }
        sidecar_paths.sort();

        let mut replayed = 0;
        for path in sidecar_paths {
            let raw = match std::fs::read(&path) {
                Ok(raw) => raw,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("failed to read pending terminal sidecar {}", path.display())
                    });
                }
            };
            let pending: PendingTerminalTransition = match serde_json::from_slice(&raw) {
                Ok(parsed) => parsed,
                Err(error) => {
                    warn!(
                        path = %path.display(),
                        %error,
                        "Skipping malformed pending terminal sidecar"
                    );
                    continue;
                }
            };
            if !pending.status.is_terminal() {
                warn!(
                    job_id = %pending.job_id,
                    status = ?pending.status,
                    path = %path.display(),
                    "Skipping non-terminal sidecar record"
                );
                continue;
            }

            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let updated = tx
                .execute(
                    "UPDATE jobs SET
                        status = ?2,
                        completed_at = ?3,
                        error = ?4,
                        updated_at = ?5,
                        cancel_requested_at = COALESCE(?6, cancel_requested_at),
                        cancel_reason = COALESCE(?7, cancel_reason)
                     WHERE id = ?1",
                    params![
                        pending.job_id,
                        status_to_str(pending.status),
                        pending.completed_at.to_rfc3339(),
                        pending.error,
                        Utc::now().to_rfc3339(),
                        pending.cancel_requested_at.map(|ts| ts.to_rfc3339()),
                        pending.cancel_reason.map(cancel_reason_to_str),
                    ],
                )
                .with_context(|| {
                    format!(
                        "failed to replay pending terminal transition for job {}",
                        pending.job_id
                    )
                })?;
            tx.execute(
                "DELETE FROM job_staged_files WHERE job_id = ?1",
                params![pending.job_id],
            )?;
            tx.commit()?;
            if updated > 0 {
                replayed += 1;
            }
            if let Err(error) = std::fs::remove_file(&path) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    warn!(
                        path = %path.display(),
                        %error,
                        "Failed to remove replayed pending terminal sidecar"
                    );
                }
            }
        }
        Ok(replayed)
    }

    pub(crate) fn delete_job(&self, job_id: &str) -> Result<usize> {
        let deleted_rows = self.with_connection(|conn| {
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let deleted_rows = tx
                .execute("DELETE FROM jobs WHERE id = ?1", params![job_id])
                .with_context(|| format!("failed to delete persisted job {job_id}"))?;
            tx.execute(
                "DELETE FROM job_staged_files WHERE job_id = ?1",
                params![job_id],
            )?;
            tx.commit()?;
            Ok(deleted_rows)
        })?;
        self.remove_pending_terminal_sidecar(job_id);
        Ok(deleted_rows)
    }

    /// Removes a persisted row that is not present in runtime `jobs`, refusing
    /// to touch pre-acceptance `admitting` rows that are currently in flight.
    pub(crate) fn delete_unloaded_job(&self, job_id: &str) -> Result<usize> {
        let deleted_rows = self.with_connection(|conn| {
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let deleted_rows = tx
                .execute(
                    "DELETE FROM jobs WHERE id = ?1 AND status != ?2",
                    params![job_id, STATUS_ADMITTING],
                )
                .with_context(|| format!("failed to delete unloaded job {job_id}"))?;
            if deleted_rows > 0 {
                tx.execute(
                    "DELETE FROM job_staged_files WHERE job_id = ?1",
                    params![job_id],
                )?;
            }
            tx.commit()?;
            Ok(deleted_rows)
        })?;
        if deleted_rows > 0 {
            self.remove_pending_terminal_sidecar(job_id);
        }
        Ok(deleted_rows)
    }

    fn initialize_schema(&self) -> Result<()> {
        self.with_connection(|conn| {
            conn.execute_batch("PRAGMA journal_mode = WAL;")
                .with_context(|| format!("failed to open jobs db: {}", self.db_path.display()))?;
            // Pending migrations run in order in one transaction: a new
            // database costs one commit, and a failed step leaves the file as
            // it was.
            let transaction =
                conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let version: i64 =
                transaction.query_row("PRAGMA user_version", [], |row| row.get(0))?;
            if version > SCHEMA_VERSION {
                bail!(
                    "jobs db {} has schema version {version}, newer than this build supports \
                     ({SCHEMA_VERSION}); run a newer Videnoa or move the file away",
                    self.db_path.display()
                );
            }
            if version < 1 {
                self.migrate_to_v1(&transaction)?;
            }
            if version < 2 {
                Self::migrate_to_v2(&transaction).with_context(|| {
                    format!(
                        "failed to migrate jobs db to schema version 2: {}",
                        self.db_path.display()
                    )
                })?;
            }
            if version < 3 {
                Self::migrate_to_v3(&transaction).with_context(|| {
                    format!(
                        "failed to migrate jobs db to schema version 3: {}",
                        self.db_path.display()
                    )
                })?;
            }
            if version < 4 {
                Self::migrate_to_v4(&transaction).with_context(|| {
                    format!(
                        "failed to migrate jobs db to schema version 4: {}",
                        self.db_path.display()
                    )
                })?;
            }
            transaction.commit()?;
            Ok(())
        })
    }

    fn migrate_to_v1(&self, transaction: &rusqlite::Transaction<'_>) -> Result<()> {
        transaction
            .execute_batch(
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
        Self::migrate_idempotency_schema(transaction).map_err(|error| {
            anyhow::anyhow!(
                "failed to migrate jobs idempotency schema at {}: {error:#}",
                self.db_path.display()
            )
        })?;
        transaction.execute_batch("PRAGMA user_version = 1;")?;
        Ok(())
    }

    fn migrate_to_v2(transaction: &rusqlite::Transaction<'_>) -> Result<()> {
        let columns = table_columns(transaction)?;
        for column in ["cancel_requested_at", "cancel_reason"] {
            if !columns.iter().any(|existing| existing == column) {
                transaction.execute(&format!("ALTER TABLE jobs ADD COLUMN {column} TEXT"), [])?;
            }
        }
        transaction.execute_batch("PRAGMA user_version = 2;")?;
        Ok(())
    }

    fn migrate_to_v3(transaction: &rusqlite::Transaction<'_>) -> Result<()> {
        transaction.execute_batch(
            "CREATE TABLE IF NOT EXISTS job_staged_files (
                job_id TEXT NOT NULL,
                path TEXT NOT NULL,
                PRIMARY KEY (job_id, path)
             );
             CREATE INDEX IF NOT EXISTS idx_job_staged_files_job_id
                 ON job_staged_files(job_id);
             PRAGMA user_version = 3;",
        )?;
        Ok(())
    }

    fn migrate_to_v4(transaction: &rusqlite::Transaction<'_>) -> Result<()> {
        let mut statement = transaction.prepare("PRAGMA table_info(job_staged_files)")?;
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);
        for column in ["dir_dev", "dir_ino"] {
            if !columns.iter().any(|existing| existing == column) {
                transaction.execute(
                    &format!("ALTER TABLE job_staged_files ADD COLUMN {column} INTEGER"),
                    [],
                )?;
            }
        }
        transaction.execute_batch("PRAGMA user_version = 4;")?;
        Ok(())
    }

    fn with_connection<T>(&self, op: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T> {
        // A panic while holding the lock leaves no open transaction behind
        // (rusqlite rolls back on drop), so the connection stays usable.
        let mut conn = self
            .connection
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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

pub(crate) fn capture_parent_dir_identity(path: &Path) -> Option<StagedDirIdentity> {
    let dir = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    capture_dir_identity(dir)
}

#[cfg(unix)]
pub(crate) fn capture_dir_identity(dir: &Path) -> Option<StagedDirIdentity> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::metadata(dir).ok()?;
    if !metadata.is_dir() {
        return None;
    }
    Some(StagedDirIdentity {
        dev: metadata.dev(),
        ino: metadata.ino(),
    })
}

#[cfg(not(unix))]
pub(crate) fn capture_dir_identity(_dir: &Path) -> Option<StagedDirIdentity> {
    None
}
