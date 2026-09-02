use std::sync::Arc;

use tokio::task::JoinSet;
use videnoa_controller::scheduler::{AssignmentClass, Scheduler};

use super::support::{
    create_worker_with_id, fixture, online, task, task_id, timestamp, worker_id, worker_request,
    TestResult,
};

#[tokio::test]
async fn scheduler_selects_exact_task_and_worker_order_with_compatibility() -> TestResult {
    // Given: mixed-priority tasks and eligible, incompatible, offline, and disabled workers.
    let fixture = fixture().await?;
    let incompatible = create_worker_with_id(
        &fixture,
        worker_id(1),
        worker_request("incompatible", "https://incompatible.example/api/", 1)?,
    )
    .await?;
    online(&fixture, incompatible.id, incompatible.version, &["other"]).await?;
    let first_worker = create_worker_with_id(
        &fixture,
        worker_id(2),
        worker_request("worker-a", "https://worker-a.example/api/", 1)?,
    )
    .await?;
    online(
        &fixture,
        first_worker.id,
        first_worker.version,
        &["anime-upscale"],
    )
    .await?;
    let second_worker = create_worker_with_id(
        &fixture,
        worker_id(3),
        worker_request("worker-b", "https://worker-b.example/api/", 1)?,
    )
    .await?;
    online(
        &fixture,
        second_worker.id,
        second_worker.version,
        &["anime-upscale"],
    )
    .await?;
    let offline = create_worker_with_id(
        &fixture,
        worker_id(4),
        worker_request("offline", "https://offline.example/api/", 1)?,
    )
    .await?;
    let disabled = create_worker_with_id(
        &fixture,
        worker_id(5),
        worker_request("disabled", "https://disabled.example/api/", 1)?,
    )
    .await?;
    online(&fixture, disabled.id, disabled.version, &["anime-upscale"]).await?;
    let disabled = fixture
        .registry
        .worker(disabled.id)
        .await?
        .ok_or_else(|| std::io::Error::other("disabled worker missing"))?;
    fixture
        .registry
        .set_enabled(disabled.id, disabled.version, false, fixture.now)
        .await?;
    assert!(!offline.online);
    let oldest = timestamp(1_788_307_190)?;
    let older_high = task_id(10);
    let newer_high = task_id(11);
    let lower = task_id(12);
    fixture
        .store
        .insert_task(&task(older_high, "anime-upscale", 20, oldest))
        .await?;
    fixture
        .store
        .insert_task(&task(newer_high, "anime-upscale", 20, fixture.now))
        .await?;
    fixture
        .store
        .insert_task(&task(lower, "anime-upscale", 10, oldest))
        .await?;
    let scheduler = Scheduler::load(fixture.store.clone()).await?;

    // When: two assignments are selected.
    let first = scheduler
        .reserve_next(fixture.now)
        .await?
        .ok_or_else(|| std::io::Error::other("first assignment missing"))?;
    let second = scheduler
        .reserve_next(fixture.now)
        .await?
        .ok_or_else(|| std::io::Error::other("second assignment missing"))?;

    // Then: task priority precedes age/ID and worker usage precedes assignment time/ID.
    assert_eq!(first.task_id(), older_high);
    assert_eq!(first.worker_id(), first_worker.id);
    assert_eq!(first.class(), AssignmentClass::IdleFeed);
    assert_eq!(second.task_id(), newer_high);
    assert_eq!(second.worker_id(), second_worker.id);
    assert_eq!(second.class(), AssignmentClass::IdleFeed);
    assert_eq!(fixture.store.worker_used_slots(incompatible.id).await?, 0);
    assert_eq!(fixture.store.worker_used_slots(offline.id).await?, 0);
    assert_eq!(fixture.store.worker_used_slots(disabled.id).await?, 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_scheduler_claims_never_exceed_capacity_or_duplicate_attempts() -> TestResult {
    // Given: one two-slot worker and four queued tasks.
    let fixture = fixture().await?;
    let worker = fixture
        .registry
        .create(
            worker_request("worker-a", "https://worker.example/api/", 2)?,
            fixture.now,
        )
        .await?;
    online(&fixture, worker.id, worker.version, &["anime-upscale"]).await?;
    let task_ids = [task_id(21), task_id(22), task_id(23), task_id(24)];
    for id in task_ids {
        fixture
            .store
            .insert_task(&task(id, "anime-upscale", 10, fixture.now))
            .await?;
    }
    let scheduler = Arc::new(Scheduler::load(fixture.store.clone()).await?);
    let reservation_time = timestamp(1_788_307_200)?;

    // When: eight writers race deterministic selection and atomic reservation.
    let mut writes = JoinSet::new();
    for _ in 0..8 {
        let scheduler = Arc::clone(&scheduler);
        writes.spawn(async move { scheduler.reserve_next(reservation_time).await });
    }
    let mut assignments = Vec::new();
    while let Some(joined) = writes.join_next().await {
        if let Some(assignment) = joined?? {
            assignments.push(assignment);
        }
    }

    // Then: exactly two unique task/attempt pairs own the two durable slots.
    assignments.sort_by_key(|assignment| assignment.task_id());
    assignments.dedup_by_key(|assignment| assignment.task_id());
    assert_eq!(assignments.len(), 2);
    assert_eq!(fixture.store.worker_used_slots(worker.id).await?, 2);
    assert_eq!(fixture.store.count_attempts_for_tasks(&task_ids).await?, 2);
    Ok(())
}
