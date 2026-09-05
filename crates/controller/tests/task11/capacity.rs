use videnoa_controller::domain::{ComputeSlots, TaskStatus};
use videnoa_controller::lifecycle::{AdvanceCommand, LifecycleErrorCode, LifecycleService};
use videnoa_controller::persistence::{SettingsUpdate, WorkerUpdate, WorkerUpdateOutcome};
use videnoa_controller::scheduler::Scheduler;

use self::support::{reserve, set_status, snapshots};
use super::support::{fixture, online, task, task_id, worker_request, TestResult};

#[path = "capacity/support.rs"]
mod support;

#[tokio::test]
async fn reservation_budget_covers_idle_compute_demand_plus_prefetch() -> TestResult {
    // Given: a two-slot worker, configured prefetch one, and four queued tasks.
    let fixture = fixture().await?;
    let worker = fixture
        .registry
        .create(
            worker_request("worker-a", "https://worker.example/api/", 2)?,
            fixture.now,
        )
        .await?;
    online(&fixture, worker.id, worker.version, &["anime-upscale"]).await?;
    for id in [task_id(501), task_id(502), task_id(503), task_id(504)] {
        fixture
            .store
            .insert_task(&task(id, "anime-upscale", 10, fixture.now))
            .await?;
    }
    let scheduler = Scheduler::load(fixture.store.clone())?;

    // When: reservation fills idle compute demand and the configured prefetch budget.
    let first = scheduler.reserve_next(fixture.now).await?;
    let second = scheduler.reserve_next(fixture.now).await?;
    let third = scheduler.reserve_next(fixture.now).await?;
    let bounded = scheduler.reserve_next(fixture.now).await?;

    // Then: three tasks are staged in, while the fourth remains queued.
    assert!(first.is_some());
    assert!(second.is_some());
    assert!(third.is_some());
    assert!(bounded.is_none());
    assert_eq!(fixture.store.worker_used_slots(worker.id).await?, 0);
    Ok(())
}

#[tokio::test]
async fn overcommitted_compute_preserves_nonnegative_prefetch_budget() -> TestResult {
    // Given: durable compute exceeds a one-slot worker and prefetch remains configured to one.
    let fixture = fixture().await?;
    let worker = fixture
        .registry
        .create(
            worker_request("worker-a", "https://worker.example/api/", 1)?,
            fixture.now,
        )
        .await?;
    online(&fixture, worker.id, worker.version, &["anime-upscale"]).await?;
    for id in [task_id(505), task_id(506)] {
        fixture
            .store
            .insert_task(&task(id, "anime-upscale", 20, fixture.now))
            .await?;
        reserve(&fixture, id, worker.id).await?;
        set_status(&fixture, id, TaskStatus::Processing).await?;
    }
    for id in [task_id(507), task_id(508)] {
        fixture
            .store
            .insert_task(&task(id, "anime-upscale", 10, fixture.now))
            .await?;
    }
    let scheduler = Scheduler::load(fixture.store.clone())?;

    // When: reservation evaluates the overcommitted durable worker state.
    let prefetched = scheduler.reserve_next(fixture.now).await?;
    let bounded = scheduler.reserve_next(fixture.now).await?;

    // Then: negative idle demand is clamped to zero and exactly prefetch one remains available.
    assert!(prefetched.is_some());
    assert!(bounded.is_none());
    assert_eq!(fixture.store.worker_used_slots(worker.id).await?, 2);
    Ok(())
}

#[tokio::test]
async fn compute_capacity_counts_only_submitting_and_processing() -> TestResult {
    // Given: one assigned task whose durable state can be inspected at every pipeline stage.
    let fixture = fixture().await?;
    let worker = fixture
        .registry
        .create(
            worker_request("worker-a", "https://worker.example/api/", 1)?,
            fixture.now,
        )
        .await?;
    online(&fixture, worker.id, worker.version, &["anime-upscale"]).await?;
    let task_id = task_id(511);
    fixture
        .store
        .insert_task(&task(task_id, "anime-upscale", 10, fixture.now))
        .await?;
    reserve(&fixture, task_id, worker.id).await?;

    // When: the task advances through stage-in, compute, and downstream states.
    for (status, expected_used) in [
        (TaskStatus::Reserved, 0),
        (TaskStatus::Uploading, 0),
        (TaskStatus::Staged, 0),
        (TaskStatus::Submitting, 1),
        (TaskStatus::Processing, 1),
        (TaskStatus::RemoteCompleted, 0),
        (TaskStatus::Downloading, 0),
        (TaskStatus::Verifying, 0),
        (TaskStatus::Publishing, 0),
        (TaskStatus::RemoteCleanup, 0),
    ] {
        set_status(&fixture, task_id, status).await?;

        // Then: only remote compute ownership consumes the worker slot.
        let capacity = fixture.registry.capacity(worker.id).await?;
        assert_eq!(capacity.used_slots, expected_used, "status={status:?}");
        assert_eq!(
            capacity.available_slots,
            1 - expected_used,
            "status={status:?}"
        );
        assert_eq!(
            fixture.store.worker_used_slots(worker.id).await?,
            u64::from(expected_used),
            "status={status:?}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn staged_submission_atomically_claims_compute_capacity_and_pause() -> TestResult {
    // Given: two staged tasks prefetched for one compute slot.
    let fixture = fixture().await?;
    let worker = fixture
        .registry
        .create(
            worker_request("worker-a", "https://worker.example/api/", 1)?,
            fixture.now,
        )
        .await?;
    online(&fixture, worker.id, worker.version, &["anime-upscale"]).await?;
    let first_id = task_id(521);
    let second_id = task_id(522);
    for id in [first_id, second_id] {
        fixture
            .store
            .insert_task(&task(id, "anime-upscale", 10, fixture.now))
            .await?;
        reserve(&fixture, id, worker.id).await?;
        set_status(&fixture, id, TaskStatus::Staged).await?;
    }
    let service = LifecycleService::new(fixture.store.clone());
    let (first_task, first_attempt) = snapshots(&fixture, first_id).await?;
    let (second_task, second_attempt) = snapshots(&fixture, second_id).await?;

    // When: both staged tasks attempt to claim the single compute slot.
    service
        .advance(
            &first_task,
            &first_attempt,
            AdvanceCommand::StartSubmission,
            fixture.now,
        )
        .await?;
    let capacity_error = service
        .advance(
            &second_task,
            &second_attempt,
            AdvanceCommand::StartSubmission,
            fixture.now,
        )
        .await
        .expect_err("second staged task must not overclaim compute");

    // Then: the loser stays staged, and pause also rejects a later atomic claim.
    assert_eq!(capacity_error.code(), LifecycleErrorCode::Conflict);
    assert_eq!(
        fixture
            .store
            .task(second_id)
            .await?
            .ok_or_else(|| {
                std::io::Error::other("second task missing after capacity conflict")
            })?
            .status,
        TaskStatus::Staged
    );
    set_status(&fixture, first_id, TaskStatus::RemoteCompleted).await?;
    let settings = fixture.store.config_manager().settings()?;
    let mut scheduler = settings.scheduler;
    scheduler.paused = true;
    fixture
        .store
        .config_manager()
        .update_settings(&SettingsUpdate {
            expected_version: settings.version,
            scheduler,
            timeouts: settings.timeouts,
            retry: settings.retry,
            updated_at: fixture.now,
        })
        .await?;
    let (second_task, second_attempt) = snapshots(&fixture, second_id).await?;
    let pause_error = service
        .advance(
            &second_task,
            &second_attempt,
            AdvanceCommand::StartSubmission,
            fixture.now,
        )
        .await
        .expect_err("paused scheduler must reject staged compute claim");
    assert_eq!(pause_error.code(), LifecycleErrorCode::Conflict);
    Ok(())
}

#[tokio::test]
async fn capacity_reduction_ignores_existing_stage_in() -> TestResult {
    // Given: two staged tasks assigned to a two-slot worker.
    let fixture = fixture().await?;
    let worker = fixture
        .registry
        .create(
            worker_request("worker-a", "https://worker.example/api/", 2)?,
            fixture.now,
        )
        .await?;
    online(&fixture, worker.id, worker.version, &["anime-upscale"]).await?;
    for id in [task_id(531), task_id(532)] {
        fixture
            .store
            .insert_task(&task(id, "anime-upscale", 10, fixture.now))
            .await?;
        reserve(&fixture, id, worker.id).await?;
        set_status(&fixture, id, TaskStatus::Staged).await?;
    }
    let worker = fixture
        .store
        .worker(worker.id)
        .await?
        .ok_or_else(|| std::io::Error::other("worker missing"))?;

    // When: compute capacity is reduced while stage-in remains prefetched.
    let outcome = fixture
        .store
        .update_worker(&WorkerUpdate {
            id: worker.id,
            expected_version: worker.version,
            name: worker.name,
            api_url: worker.api_url,
            enabled: worker.enabled,
            compute_slots: ComputeSlots::try_from(1_u64)?,
            updated_at: fixture.now,
        })
        .await?;

    // Then: only active compute constrains the reduction.
    assert!(matches!(outcome, WorkerUpdateOutcome::Applied { .. }));
    assert_eq!(fixture.store.worker_used_slots(worker.id).await?, 0);
    Ok(())
}
