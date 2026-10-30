use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

use super::{JobsPersistence, SCHEMA_VERSION};
use crate::graph::PipelineGraph;
use crate::server::{CancelReason, CancelRequest, Job, JobStatus};

fn job(id: &str, status: JobStatus) -> Job {
    Job {
        id: id.to_owned(),
        status,
        workflow: PipelineGraph::new(),
        created_at: Utc::now(),
        started_at: None,
        completed_at: None,
        progress: None,
        error: None,
        cancel_token: CancellationToken::new(),
        params: None,
        workflow_name: "schema-test".to_owned(),
        workflow_source: "api_jobs".to_owned(),
        rerun_of_job_id: None,
        cancel_request: None,
    }
}

fn user_version(data_dir: &std::path::Path) -> Result<i64> {
    let connection = Connection::open(data_dir.join("jobs.db"))?;
    Ok(connection.query_row("PRAGMA user_version", [], |row| row.get(0))?)
}

#[test]
fn a_new_jobs_database_records_the_current_schema_version() -> Result<()> {
    let dir = TempDir::new()?;
    JobsPersistence::new(dir.path())?;
    assert_eq!(user_version(dir.path())?, SCHEMA_VERSION);
    // Opening it again is a no-op.
    JobsPersistence::new(dir.path())?;
    assert_eq!(user_version(dir.path())?, SCHEMA_VERSION);
    Ok(())
}

#[test]
fn an_unversioned_database_is_migrated_in_order_and_keeps_its_rows() -> Result<()> {
    // Given: a database from before versioning (user_version 0) that already
    // has the idempotency columns and one finished job.
    let dir = TempDir::new()?;
    let connection = Connection::open(dir.path().join("jobs.db"))?;
    connection.execute_batch(
        "CREATE TABLE jobs (
            id TEXT PRIMARY KEY, status TEXT NOT NULL, workflow_json TEXT NOT NULL,
            created_at TEXT NOT NULL, started_at TEXT, completed_at TEXT,
            progress_json TEXT, error TEXT, params_json TEXT,
            workflow_name TEXT NOT NULL, workflow_source TEXT NOT NULL,
            rerun_of_job_id TEXT, updated_at TEXT NOT NULL,
            idempotency_key TEXT, request_fingerprint TEXT
         );
         INSERT INTO jobs (id, status, workflow_json, created_at, workflow_name,
                           workflow_source, updated_at)
         VALUES ('old-job', 'completed', '{\"nodes\":[],\"connections\":[]}',
                 '2026-10-01T00:00:00+00:00', 'old', 'api_jobs', '2026-10-01T00:00:00+00:00');",
    )?;
    drop(connection);

    // When: this build opens it.
    let persistence = JobsPersistence::new(dir.path())?;

    // Then: it is at the current version and the row is intact.
    assert_eq!(user_version(dir.path())?, SCHEMA_VERSION);
    let restored = persistence.load_jobs_for_startup()?;
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].id, "old-job");
    assert_eq!(restored[0].status, JobStatus::Completed);
    assert!(restored[0].cancel_request.is_none());
    Ok(())
}

#[test]
fn a_database_from_a_newer_build_is_refused_untouched() -> Result<()> {
    let dir = TempDir::new()?;
    JobsPersistence::new(dir.path())?;
    let connection = Connection::open(dir.path().join("jobs.db"))?;
    connection.execute_batch("PRAGMA user_version = 99;")?;
    drop(connection);

    let error = JobsPersistence::new(dir.path()).expect_err("a newer schema must be refused");

    let message = format!("{error:#}");
    assert!(message.contains("schema version 99"), "{message}");
    assert_eq!(user_version(dir.path())?, 99);
    Ok(())
}

#[test]
fn a_user_cancel_survives_a_restart_with_its_reason() -> Result<()> {
    // Given: a job cancelled by a user, and one that was still running when
    // the worker stopped after a user had asked it to stop.
    let dir = TempDir::new()?;
    let persistence = JobsPersistence::new(dir.path())?;
    let requested_at = Utc::now() - chrono::Duration::seconds(30);
    let mut cancelled = job("user-cancelled", JobStatus::Cancelled);
    cancelled.cancel_request = Some(CancelRequest {
        requested_at,
        reason: CancelReason::User,
    });
    cancelled.completed_at = Some(Utc::now());
    persistence.upsert_job(&cancelled)?;
    let mut stopping = job("user-stopping", JobStatus::Running);
    persistence.upsert_job(&stopping)?;
    stopping.cancel_request = cancelled.cancel_request;
    persistence.update_job(&stopping)?;
    persistence.upsert_job(&job("interrupted", JobStatus::Running))?;

    // When: the worker restarts.
    let restored = JobsPersistence::new(dir.path())?.load_jobs_for_startup()?;

    // Then: user cancels keep their reason and time; the interrupted job is
    // cancelled for the restart.
    let find = |id: &str| restored.iter().find(|job| job.id == id).unwrap();
    for id in ["user-cancelled", "user-stopping"] {
        let request = find(id)
            .cancel_request
            .expect("cancel request is persisted");
        assert_eq!(request.reason, CancelReason::User, "{id}");
        assert_eq!(
            request.requested_at.timestamp_millis(),
            requested_at.timestamp_millis(),
            "{id}"
        );
        assert_eq!(find(id).status, JobStatus::Cancelled);
    }
    let interrupted = find("interrupted");
    assert_eq!(interrupted.status, JobStatus::Cancelled);
    assert_eq!(
        interrupted.cancel_request.map(|request| request.reason),
        Some(CancelReason::Restart)
    );
    Ok(())
}

#[test]
fn rows_that_cannot_be_loaded_are_skipped_but_can_be_deleted() -> Result<()> {
    let dir = TempDir::new()?;
    let persistence = JobsPersistence::new(dir.path())?;
    persistence.upsert_job(&job("good", JobStatus::Completed))?;
    let connection = Connection::open(dir.path().join("jobs.db"))?;
    connection.execute_batch(
        "INSERT INTO jobs (id, status, workflow_json, created_at, workflow_name,
                           workflow_source, updated_at)
         VALUES ('bad-status', 'paused', '{}', '2026-10-01T00:00:00+00:00', 'x', 'api_jobs',
                 '2026-10-01T00:00:00+00:00'),
                ('bad-workflow', 'failed', 'not json', '2026-10-01T00:00:00+00:00', 'x',
                 'api_jobs', '2026-10-01T00:00:00+00:00');",
    )?;

    let restored = persistence.load_jobs_for_startup()?;
    assert_eq!(
        restored
            .iter()
            .map(|job| job.id.as_str())
            .collect::<Vec<_>>(),
        ["good"]
    );
    assert_eq!(persistence.delete_job("bad-status")?, 1);
    assert_eq!(persistence.delete_job("bad-workflow")?, 1);
    Ok(())
}
