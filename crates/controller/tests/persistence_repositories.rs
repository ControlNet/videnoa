use std::error::Error;

use chrono::{TimeZone, Utc};
use tempfile::TempDir;
use videnoa_controller::domain::{
    AttemptId, ComputeSlots, ConcurrencyLimit, IdempotencyKey, InputExtension, InputPath,
    OutputExtension, OutputPath, RemoteJobId, RemotePath, RetrySettingsDto, SchedulerStatus,
    SessionId, SourceReference, SubmissionKey, TaskCreateRequest, TaskId, TaskSource,
    TimeoutSettingsDto, WorkerApiUrl, WorkerId, WorkerName, WorkflowName,
};
use videnoa_controller::persistence::{
    AttemptRemoteUpdate, AuthDigest, CasOutcome, Database, DatabaseOptions, IdempotencyRecord,
    NewSession, NewTask, NewWorker, PersistenceError, Reservation, ReservationOutcome,
    SettingsUpdate, Store, WorkerUpdate,
};

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

fn timestamp(seconds: i64) -> TestResult<chrono::DateTime<Utc>> {
    Utc.timestamp_opt(seconds, 0)
        .single()
        .ok_or_else(|| std::io::Error::other("invalid test timestamp").into())
}

async fn store(max_connections: u32) -> TestResult<(TempDir, Store)> {
    let directory = TempDir::new()?;
    let database = Database::open(
        DatabaseOptions::new(directory.path().join("controller.sqlite3"))
            .with_max_connections(max_connections),
    )
    .await?;
    Ok((directory, Store::new(database)))
}

fn task(id: TaskId, now: chrono::DateTime<Utc>) -> NewTask {
    NewTask {
        id,
        request: TaskCreateRequest {
            input_path: InputPath::new("/nas/input/episode.v1.mkv"),
            output_path: OutputPath::new("/nas/output/episode.final.mp4"),
            workflow: WorkflowName::new("anime upscale ../v2"),
            priority: 17,
            source: TaskSource::Api,
            source_reference: Some(SourceReference::new("source:item/42")),
        },
        input_extension: InputExtension::new("mkv"),
        output_extension: OutputExtension::new("mp4"),
        input_size: 9_999,
        input_mtime: now,
        created_at: now,
    }
}

fn worker(id: WorkerId, now: chrono::DateTime<Utc>) -> TestResult<NewWorker> {
    Ok(NewWorker {
        id,
        name: WorkerName::new("worker-a"),
        api_url: WorkerApiUrl::parse("HTTPS://WORKER.EXAMPLE:443/api")?,
        enabled: true,
        online: true,
        compute_slots: ComputeSlots::try_from(2_u64)?,
        created_at: now,
    })
}

#[tokio::test]
async fn task_and_attempt_repositories_round_trip_remote_evidence() -> TestResult {
    // Given: a queued task and an enabled worker.
    let (_directory, store) = store(2).await?;
    let now = timestamp(1_788_307_200)?;
    let task_id = TaskId::random();
    let worker_id = WorkerId::random();
    store.insert_worker(&worker(worker_id, now)?).await?;
    store.insert_task(&task(task_id, now)).await?;
    let attempt_id = AttemptId::random();
    let reservation = Reservation {
        task_id,
        expected_task_version: 0,
        worker_id,
        attempt_id,
        submission_key: SubmissionKey::random(),
        reserved_at: now,
    };

    // When: reservation and remote reconciliation evidence are persisted.
    assert_eq!(
        store.reserve_task(&reservation).await?,
        ReservationOutcome::Reserved(attempt_id)
    );
    let remote_job_id = RemoteJobId::random();
    assert_eq!(
        store
            .update_attempt_remote(&AttemptRemoteUpdate {
                attempt_id,
                expected_version: 0,
                remote_job_id,
                remote_input_path: RemotePath::new("task/input/../opaque.mkv"),
                remote_output_path: RemotePath::new("task/output/../opaque.mp4"),
                submitted_at: now,
            })
            .await?,
        CasOutcome::Applied { new_version: 1 }
    );

    // Then: immutable paths, snapshots, assignment, attempt number, and remote paths round-trip.
    let stored_task = store
        .task(task_id)
        .await?
        .ok_or_else(|| std::io::Error::other("task missing"))?;
    let stored_attempt = store
        .attempt(attempt_id)
        .await?
        .ok_or_else(|| std::io::Error::other("attempt missing"))?;
    assert_eq!(stored_task.request, task(task_id, now).request);
    assert_eq!(stored_task.input_size, 9_999);
    assert_eq!(stored_task.worker_id, Some(worker_id));
    assert_eq!(stored_attempt.attempt.attempt_number, 1);
    assert_eq!(stored_attempt.attempt.remote_job_id, Some(remote_job_id));
    assert_eq!(
        stored_attempt
            .attempt
            .remote_input_path
            .as_ref()
            .map(RemotePath::as_str),
        Some("task/input/../opaque.mkv")
    );
    Ok(())
}

#[tokio::test]
async fn worker_and_settings_updates_reject_stale_versions() -> TestResult {
    // Given: one worker and singleton default settings at version zero.
    let (_directory, store) = store(2).await?;
    let now = timestamp(1_788_307_200)?;
    let worker_id = WorkerId::random();
    store.insert_worker(&worker(worker_id, now)?).await?;
    let worker_update = WorkerUpdate {
        id: worker_id,
        expected_version: 0,
        name: WorkerName::new("worker-renamed"),
        api_url: WorkerApiUrl::parse("https://worker.example/new/")?,
        enabled: false,
        compute_slots: ComputeSlots::try_from(3_u64)?,
        updated_at: now,
    };
    let settings = store.settings().await?;
    let settings_update = SettingsUpdate {
        expected_version: settings.version,
        scheduler: SchedulerStatus {
            paused: true,
            default_compute_slots: ComputeSlots::try_from(2_u64)?,
            prefetch_per_worker: 0,
            max_concurrent_uploads: ConcurrencyLimit::try_from(2_u64)?,
            max_concurrent_downloads: ConcurrencyLimit::try_from(3_u64)?,
        },
        timeouts: TimeoutSettingsDto {
            health_seconds: 11,
            poll_seconds: 6,
            transfer_seconds: 301,
        },
        retry: RetrySettingsDto {
            initial_seconds: 2,
            maximum_seconds: 61,
            max_attempts: 6,
        },
        updated_at: now,
    };

    // When: each optimistic update is replayed with its stale version.
    assert_eq!(
        store.update_worker(&worker_update).await?,
        CasOutcome::Applied { new_version: 1 }
    );
    assert_eq!(
        store.update_worker(&worker_update).await?,
        CasOutcome::Conflict
    );
    assert_eq!(
        store.update_settings(&settings_update).await?,
        CasOutcome::Applied { new_version: 1 }
    );
    assert_eq!(
        store.update_settings(&settings_update).await?,
        CasOutcome::Conflict
    );

    // Then: only the first values are durable.
    assert_eq!(
        store
            .worker(worker_id)
            .await?
            .ok_or_else(|| std::io::Error::other("worker missing"))?
            .name
            .as_str(),
        "worker-renamed"
    );
    assert!(store.settings().await?.scheduler.paused);
    Ok(())
}

#[tokio::test]
async fn session_and_idempotency_repositories_store_only_digests_and_fingerprints() -> TestResult {
    // Given: a task, three fixed digests, and bounded session expiries.
    let (_directory, store) = store(2).await?;
    let now = timestamp(1_788_307_200)?;
    let task_id = TaskId::random();
    store.insert_task(&task(task_id, now)).await?;
    let session = NewSession {
        id: SessionId::random(),
        token_digest: AuthDigest::new([1; 32]),
        csrf_digest: AuthDigest::new([2; 32]),
        password_hash_fingerprint: AuthDigest::new([3; 32]),
        absolute_expires_at: timestamp(1_788_393_600)?,
        idle_expires_at: timestamp(1_788_310_800)?,
        created_at: now,
    };
    let idempotency = IdempotencyRecord {
        key: IdempotencyKey::new("source:episode/42"),
        request_fingerprint: [4; 32],
        task_id,
        created_at: now,
    };

    // When: both authentication and ingress recovery records are inserted and loaded.
    store.insert_session(&session).await?;
    store.insert_task_idempotency(&idempotency).await?;
    let stored_session = store
        .session_by_token_digest(session.token_digest)
        .await?
        .ok_or_else(|| std::io::Error::other("session missing"))?;
    let stored_idempotency = store
        .task_idempotency(&idempotency.key)
        .await?
        .ok_or_else(|| std::io::Error::other("idempotency row missing"))?;

    // Then: fixed-size digests and the opaque key round-trip without plaintext columns.
    assert_eq!(stored_session.id, session.id);
    assert_eq!(stored_session.csrf_digest.as_bytes(), &[2; 32]);
    assert_eq!(stored_idempotency, idempotency);
    Ok(())
}

#[tokio::test]
async fn constraints_reject_invalid_relations_and_unknown_persisted_enums_are_typed_errors(
) -> TestResult {
    // Given: one valid task in a single-connection database.
    let (_directory, store) = store(1).await?;
    let now = timestamp(1_788_307_200)?;
    let task_id = TaskId::random();
    store.insert_task(&task(task_id, now)).await?;

    // When: a missing task relation and a deliberately corrupted enum cross the boundary.
    let foreign_key = sqlx::query(
        "INSERT INTO task_attempts (
            id, task_id, attempt_no, status, submission_key, progress_json, created_at_ms, updated_at_ms
         ) VALUES (?, ?, 1, 'queued', ?, '{\"percent\":0}', ?, ?)",
    )
    .bind(AttemptId::random().to_string())
    .bind(TaskId::random().to_string())
    .bind(SubmissionKey::random().to_string())
    .bind(now.timestamp_millis())
    .bind(now.timestamp_millis())
    .execute(store.database().pool())
    .await;
    sqlx::query("PRAGMA ignore_check_constraints = ON")
        .execute(store.database().pool())
        .await?;
    sqlx::query("UPDATE tasks SET status = 'future_state' WHERE id = ?")
        .bind(task_id.to_string())
        .execute(store.database().pool())
        .await?;
    let corrupted = store.task(task_id).await;

    // Then: SQLite rejects the relation and row mapping rejects the unknown enum without coercion.
    assert!(foreign_key.is_err());
    assert!(matches!(
        corrupted,
        Err(PersistenceError::CorruptValue {
            field: "status",
            ..
        })
    ));
    Ok(())
}
