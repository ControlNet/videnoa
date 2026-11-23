use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

use super::{JobsPersistence, PendingTerminalTransition, SCHEMA_VERSION};
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

#[test]
fn pending_terminal_sidecar_is_replayed_before_running_jobs_are_cancelled() -> Result<()> {
    let dir = TempDir::new()?;
    let persistence = JobsPersistence::new(dir.path())?;
    let mut running = job("sidecar-completed", JobStatus::Running);
    running.started_at = Some(Utc::now() - chrono::Duration::seconds(10));
    persistence.upsert_job(&running)?;
    persistence.record_staged_file("sidecar-completed", std::path::Path::new("/tmp/out.mkv"))?;

    let completed_at = Utc::now();
    persistence.write_pending_terminal_sidecar(&PendingTerminalTransition {
        job_id: "sidecar-completed".to_owned(),
        status: JobStatus::Completed,
        completed_at,
        error: None,
        cancel_requested_at: None,
        cancel_reason: None,
    })?;
    assert!(persistence
        .pending_terminal_sidecar_path("sidecar-completed")
        .exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode =
            std::fs::metadata(persistence.pending_terminal_sidecar_path("sidecar-completed"))?
                .permissions()
                .mode()
                & 0o777;
        assert_eq!(
            mode, 0o600,
            "pending terminal sidecar must keep owner-only permissions (0600)"
        );
    }
    drop(persistence);

    let reopened = JobsPersistence::new(dir.path())?;
    let (restored, interrupted) = reopened.load_jobs_for_startup_with_interrupted()?;
    assert!(interrupted.is_empty());
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].id, "sidecar-completed");
    assert_eq!(restored[0].status, JobStatus::Completed);
    assert_eq!(
        restored[0].completed_at.map(|ts| ts.timestamp_millis()),
        Some(completed_at.timestamp_millis())
    );
    assert!(reopened.list_staged_files()?.is_empty());
    assert!(!reopened
        .pending_terminal_sidecar_path("sidecar-completed")
        .exists());
    Ok(())
}

#[test]
fn staged_file_records_persist_across_running_reconciliation_until_cleared() -> Result<()> {
    let dir = TempDir::new()?;
    let persistence = JobsPersistence::new(dir.path())?;
    persistence.upsert_job(&job("interrupted-encode", JobStatus::Running))?;
    let staged_target = dir.path().join("computed").join("Episode 01.mkv");
    persistence.record_staged_file("interrupted-encode", &staged_target)?;
    drop(persistence);

    let reopened = JobsPersistence::new(dir.path())?;
    let (_, interrupted) = reopened.load_jobs_for_startup_with_interrupted()?;
    assert_eq!(interrupted, vec!["interrupted-encode".to_owned()]);
    assert_eq!(
        reopened.list_staged_files()?,
        vec![("interrupted-encode".to_owned(), staged_target.clone())]
    );
    reopened.remove_staged_file_record("interrupted-encode", &staged_target)?;
    assert!(reopened.list_staged_files()?.is_empty());
    Ok(())
}

#[test]
fn record_cancel_request_and_non_terminal_update_never_clobber_terminal_or_prior_cancel(
) -> Result<()> {
    let dir = TempDir::new()?;
    let persistence = JobsPersistence::new(dir.path())?;

    let mut active = job("active-job", JobStatus::Running);
    active.started_at = Some(Utc::now() - chrono::Duration::seconds(5));
    persistence.upsert_job(&active)?;

    let user_cancel = CancelRequest {
        requested_at: Utc::now() - chrono::Duration::seconds(2),
        reason: CancelReason::User,
    };
    assert!(persistence.record_cancel_request("active-job", &user_cancel)?);

    // A later shutdown cancel request must not overwrite the user's cancel reason.
    let shutdown_cancel = CancelRequest {
        requested_at: Utc::now(),
        reason: CancelReason::Shutdown,
    };
    assert!(!persistence.record_cancel_request("active-job", &shutdown_cancel)?);

    // A completed job must ignore both record_cancel_request and stale non-terminal update_job.
    let mut completed = job("completed-job", JobStatus::Completed);
    completed.started_at = Some(Utc::now() - chrono::Duration::seconds(10));
    completed.completed_at = Some(Utc::now() - chrono::Duration::seconds(1));
    persistence.upsert_job(&completed)?;

    assert!(!persistence.record_cancel_request("completed-job", &user_cancel)?);
    let mut stale_running = job("completed-job", JobStatus::Running);
    stale_running.cancel_request = Some(user_cancel);
    persistence.update_job(&stale_running)?;

    let (restored, interrupted) =
        JobsPersistence::new(dir.path())?.load_jobs_for_startup_with_interrupted()?;
    assert_eq!(interrupted, vec!["active-job".to_owned()]);
    let restored_active = restored.iter().find(|j| j.id == "active-job").unwrap();
    assert_eq!(restored_active.status, JobStatus::Cancelled);
    assert_eq!(
        restored_active.cancel_request.map(|r| r.reason),
        Some(CancelReason::User)
    );
    let restored_completed = restored.iter().find(|j| j.id == "completed-job").unwrap();
    assert_eq!(restored_completed.status, JobStatus::Completed);
    assert!(restored_completed.cancel_request.is_none());
    Ok(())
}

#[test]
fn delete_job_cleans_up_staged_file_records_and_sidecar_for_target_job_only() -> Result<()> {
    let dir = TempDir::new()?;
    let persistence = JobsPersistence::new(dir.path())?;
    persistence.upsert_job(&job("delete-me", JobStatus::Completed))?;
    persistence.upsert_job(&job("keep-me", JobStatus::Running))?;

    let deleted_staged = dir.path().join("media").join("deleted.mkv");
    let kept_staged = dir.path().join("media").join("kept.mkv");
    persistence.record_staged_file("delete-me", &deleted_staged)?;
    persistence.record_staged_file("keep-me", &kept_staged)?;
    persistence.write_pending_terminal_sidecar(&PendingTerminalTransition {
        job_id: "delete-me".to_owned(),
        status: JobStatus::Completed,
        completed_at: Utc::now(),
        error: None,
        cancel_requested_at: None,
        cancel_reason: None,
    })?;

    assert_eq!(persistence.delete_job("delete-me")?, 1);
    assert!(!persistence
        .pending_terminal_sidecar_path("delete-me")
        .exists());
    assert_eq!(
        persistence.list_staged_files()?,
        vec![("keep-me".to_owned(), kept_staged)]
    );
    Ok(())
}

#[test]
fn admitting_rows_are_protected_from_unloaded_delete_and_purged_if_uncommitted_at_startup(
) -> Result<()> {
    let dir = TempDir::new()?;
    let persistence = JobsPersistence::new(dir.path())?;
    persistence.insert_admitting_job(&job("in-flight-job", JobStatus::Queued))?;
    persistence.insert_admitting_job(&job("promoted-job", JobStatus::Queued))?;
    persistence.activate_admitted_jobs(&["promoted-job".to_owned()])?;

    // Unloaded delete refuses to touch a pre-acceptance `admitting` row.
    assert_eq!(persistence.delete_unloaded_job("in-flight-job")?, 0);

    // On restart, an uncommitted `admitting` row is purged cleanly while the
    // promoted `queued` row reconciles normally to `cancelled`.
    let restored = persistence.load_jobs_for_startup()?;
    assert!(restored.iter().all(|j| j.id != "in-flight-job"));
    let promoted = restored.iter().find(|j| j.id == "promoted-job").unwrap();
    assert_eq!(promoted.status, JobStatus::Cancelled);
    Ok(())
}

#[test]
fn v3_database_migrates_to_v4_and_captures_directory_identity() -> Result<()> {
    let dir = TempDir::new()?;
    let db_path = dir.path().join("jobs.db");
    {
        let conn = rusqlite::Connection::open(&db_path)?;
        conn.execute_batch(
            "CREATE TABLE jobs (
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
                request_fingerprint TEXT,
                cancel_requested_at TEXT,
                cancel_reason TEXT
             );
             CREATE TABLE job_staged_files (
                job_id TEXT NOT NULL,
                path TEXT NOT NULL,
                PRIMARY KEY (job_id, path)
             );
             INSERT INTO job_staged_files (job_id, path) VALUES ('legacy-job', '/tmp/legacy.mkv');
             PRAGMA user_version = 3;",
        )?;
    }

    let persistence = JobsPersistence::new(dir.path())?;
    let media_dir = dir.path().join("mounted-media");
    std::fs::create_dir_all(&media_dir)?;
    let tracked = media_dir.join("ep01.mkv");
    persistence.record_staged_file("fresh-job", &tracked)?;

    let records = persistence.list_staged_file_records()?;
    let legacy = records.iter().find(|r| r.job_id == "legacy-job").unwrap();
    assert_eq!(legacy.dir_identity, None);
    let fresh = records.iter().find(|r| r.job_id == "fresh-job").unwrap();
    #[cfg(unix)]
    assert!(fresh.dir_identity.is_some());
    Ok(())
}
