use std::error::Error;
use std::time::Duration;

use chrono::{TimeZone, Utc};
use tempfile::TempDir;
use tokio::task::JoinSet;
use videnoa_controller::domain::{
    AttemptId, ComputeSlots, IdempotencyKey, InputExtension, InputPath, OutputExtension,
    OutputPath, SourceReference, SubmissionKey, TaskCreateRequest, TaskId, TaskSource,
    WorkerApiUrl, WorkerCapabilities, WorkerId, WorkerName, WorkflowKind, WorkflowName,
    WorkflowSummary,
};
use videnoa_controller::lifecycle::{LifecycleErrorCode, LifecycleService};
use videnoa_controller::persistence::{
    Database, DatabaseOptions, IdempotencyRecord, InputContentIdentity, InputIdentity, NewTask,
    NewWorker, Reservation, ReservationOutcome, Store, TaskIngressOutcome, WorkerHealthUpdate,
};

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

fn timestamp(seconds: i64) -> TestResult<chrono::DateTime<Utc>> {
    Utc.timestamp_opt(seconds, 0)
        .single()
        .ok_or_else(|| std::io::Error::other("invalid test timestamp").into())
}

fn task(id: TaskId, created_at: chrono::DateTime<Utc>) -> NewTask {
    NewTask {
        id,
        request: TaskCreateRequest {
            input_path: InputPath::new(format!("/input/{id}.mkv")),
            output_path: OutputPath::new(format!("/output/{id}.mp4")),
            workflow: WorkflowName::new("anime-upscale"),
            priority: 10,
            source: TaskSource::Api,
            source_reference: Some(SourceReference::new("fixture")),
        },
        input_extension: InputExtension::new("mkv"),
        output_extension: OutputExtension::new("mp4"),
        input_size: 4_096,
        input_mtime: created_at,
        input_identity: InputIdentity::new([1; 16]),
        input_content_identity: InputContentIdentity::new([2; 16]),
        created_at,
    }
}

async fn store(max_connections: u32) -> TestResult<(TempDir, Store)> {
    let directory = TempDir::new()?;
    let database = Database::open(
        DatabaseOptions::new(directory.path().join("controller.sqlite3"))
            .with_max_connections(max_connections)
            .with_busy_timeout(Duration::from_secs(5)),
    )
    .await?;
    Ok((directory, Store::new(database)))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_reservations_claim_once_and_respect_capacity() -> TestResult {
    // Given: one single-slot worker and two queued tasks.
    let (_directory, store) = store(8).await?;
    let now = timestamp(1_788_307_200)?;
    let worker_id = WorkerId::random();
    store
        .insert_worker(&NewWorker {
            id: worker_id,
            name: WorkerName::new("worker-a"),
            api_url: WorkerApiUrl::parse("https://worker.example/api/")?,
            enabled: true,
            online: true,
            compute_slots: ComputeSlots::try_from(1_u64)?,
            created_at: now,
        })
        .await?;
    store
        .update_worker_health(&WorkerHealthUpdate {
            id: worker_id,
            expected_version: 0,
            online: true,
            capabilities: WorkerCapabilities {
                workflows: vec![WorkflowSummary {
                    name: WorkflowName::new("anime-upscale"),
                    kind: WorkflowKind::Workflow,
                }],
                refreshed_at: Some(now),
            },
            last_seen_at: Some(now),
            health_retry_count: 0,
            next_health_check_at: None,
            last_error: None,
            updated_at: now,
        })
        .await?;
    let task_ids = [TaskId::random(), TaskId::random()];
    for task_id in task_ids {
        store.insert_task(&task(task_id, now)).await?;
    }

    // When: concurrent writers reserve both tasks and also race the first task twice.
    let mut writes = JoinSet::new();
    for task_id in [task_ids[0], task_ids[0], task_ids[1]] {
        let store = store.clone();
        writes.spawn(async move {
            store
                .reserve_task(&Reservation {
                    task_id,
                    expected_task_version: 0,
                    worker_id,
                    attempt_id: AttemptId::random(),
                    submission_key: SubmissionKey::random(),
                    reserved_at: now,
                })
                .await
        });
    }
    let mut reserved = 0;
    while let Some(result) = writes.join_next().await {
        if matches!(result??, ReservationOutcome::Reserved(_)) {
            reserved += 1;
        }
    }

    // Then: one idle feed plus one prefetch reserve uniquely without claiming compute.
    assert_eq!(reserved, 2);
    assert_eq!(store.worker_used_slots(worker_id).await?, 0);
    assert_eq!(store.count_attempts_for_tasks(&task_ids).await?, 2);
    Ok(())
}

#[tokio::test]
async fn stale_transition_cannot_overwrite_newer_state() -> TestResult {
    // Given: one queued task at version zero.
    let (_directory, store) = store(2).await?;
    let now = timestamp(1_788_307_200)?;
    let task_id = TaskId::random();
    store.insert_task(&task(task_id, now)).await?;

    // When: the same compare-and-swap transition is applied twice.
    let task = store
        .task(task_id)
        .await?
        .ok_or_else(|| std::io::Error::other("task missing"))?;
    let service = LifecycleService::new(store.clone());
    let first = service.request_cancellation(&task, None, now).await;
    let second = service.request_cancellation(&task, None, now).await;

    // Then: only the first transition commits.
    assert_eq!(first?.version(), 1);
    assert_eq!(
        second.expect_err("stale command must fail").code(),
        LifecycleErrorCode::Conflict
    );
    Ok(())
}

#[tokio::test]
async fn idempotent_task_ingress_replays_without_orphaning_tasks() -> TestResult {
    // Given: two task IDs represent retries of one ingress request.
    let (_directory, store) = store(2).await?;
    let now = timestamp(1_788_307_200)?;
    let first_task_id = TaskId::random();
    let retry_task_id = TaskId::random();
    let conflicting_task_id = TaskId::random();
    let key = IdempotencyKey::new("source:episode/42");
    let fingerprint = [7_u8; 32];

    // When: the request is inserted, replayed, and then reused for different content.
    let inserted = store
        .insert_task_with_idempotency(
            &task(first_task_id, now),
            &IdempotencyRecord {
                key: key.clone(),
                request_fingerprint: fingerprint,
                task_id: first_task_id,
                created_at: now,
            },
        )
        .await?;
    let replayed = store
        .insert_task_with_idempotency(
            &task(retry_task_id, now),
            &IdempotencyRecord {
                key: key.clone(),
                request_fingerprint: fingerprint,
                task_id: retry_task_id,
                created_at: now,
            },
        )
        .await?;
    let conflicting = store
        .insert_task_with_idempotency(
            &task(conflicting_task_id, now),
            &IdempotencyRecord {
                key,
                request_fingerprint: [8_u8; 32],
                task_id: conflicting_task_id,
                created_at: now,
            },
        )
        .await?;

    // Then: only the first task remains durable and all retries classify correctly.
    assert_eq!(inserted, TaskIngressOutcome::Inserted);
    assert_eq!(replayed, TaskIngressOutcome::Replay(first_task_id));
    assert_eq!(conflicting, TaskIngressOutcome::Conflict);
    assert!(store.task(first_task_id).await?.is_some());
    assert!(store.task(retry_task_id).await?.is_none());
    Ok(())
}

#[test]
fn idempotency_key_remains_an_opaque_task_ingress_value() {
    // Given/When: a non-UUID ingress key is created.
    let key = IdempotencyKey::new("source:episode/42");

    // Then: persistence does not conflate it with submission UUIDs.
    assert_eq!(key.as_str(), "source:episode/42");
}
