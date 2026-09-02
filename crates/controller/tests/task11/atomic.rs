use tokio::sync::oneshot;
use videnoa_controller::domain::{AttemptId, ComputeSlots, SubmissionKey};
use videnoa_controller::lifecycle::{LifecycleErrorCode, LifecycleService, ReserveCommand};
use videnoa_controller::persistence::{
    Reservation, ReservationOutcome, SettingsUpdate, TaskRecord, WorkerRecord, WorkerUpdate,
    WorkerUpdateOutcome,
};

use super::support::{fixture, online, task, task_id, worker_request, Fixture, TestResult};

#[tokio::test]
async fn atomic_reservation_rechecks_persisted_pause() -> TestResult {
    // Given: an eligible task/worker pair selected before scheduling is paused.
    let fixture = fixture().await?;
    let worker = fixture
        .registry
        .create(
            worker_request("worker-a", "https://worker.example/api/", 1)?,
            fixture.now,
        )
        .await?;
    online(&fixture, worker.id, worker.version, &["anime-upscale"]).await?;
    let task_id = task_id(401);
    fixture
        .store
        .insert_task(&task(task_id, "anime-upscale", 10, fixture.now))
        .await?;
    let task = fixture
        .store
        .task(task_id)
        .await?
        .ok_or_else(|| std::io::Error::other("task missing"))?;
    let settings = fixture.store.settings().await?;
    let mut scheduler = settings.scheduler;
    scheduler.paused = true;
    fixture
        .store
        .update_settings(&SettingsUpdate {
            expected_version: settings.version,
            scheduler,
            timeouts: settings.timeouts,
            retry: settings.retry,
            updated_at: fixture.now,
        })
        .await?;

    // When: lifecycle reservation is attempted directly with the stale selection.
    let error = LifecycleService::new(fixture.store.clone())
        .reserve(&ReserveCommand {
            task_id,
            expected_task_version: task.version,
            worker_id: worker.id,
            attempt_id: AttemptId::random(),
            submission_key: SubmissionKey::random(),
            reserved_at: fixture.now,
        })
        .await
        .expect_err("persisted pause must reject atomic reservation");

    // Then: the durable predicate conflicts without creating an attempt.
    assert_eq!(error.code(), LifecycleErrorCode::Conflict);
    assert!(fixture.store.current_attempt(task_id).await?.is_none());
    Ok(())
}

#[tokio::test]
async fn atomic_reservation_rechecks_workflow_compatibility() -> TestResult {
    // Given: an online worker that is incompatible with the queued workflow.
    let fixture = fixture().await?;
    let worker = fixture
        .registry
        .create(
            worker_request("worker-a", "https://worker.example/api/", 1)?,
            fixture.now,
        )
        .await?;
    online(&fixture, worker.id, worker.version, &["other"]).await?;
    let task_id = task_id(402);
    fixture
        .store
        .insert_task(&task(task_id, "anime-upscale", 10, fixture.now))
        .await?;

    // When: lifecycle reservation is attempted directly.
    let error = LifecycleService::new(fixture.store.clone())
        .reserve(&ReserveCommand {
            task_id,
            expected_task_version: 1,
            worker_id: worker.id,
            attempt_id: AttemptId::random(),
            submission_key: SubmissionKey::random(),
            reserved_at: fixture.now,
        })
        .await
        .expect_err("incompatible workflow must reject atomic reservation");

    // Then: compatibility is enforced by the same durable claim.
    assert_eq!(error.code(), LifecycleErrorCode::Conflict);
    assert!(fixture.store.current_attempt(task_id).await?.is_none());
    Ok(())
}

#[tokio::test]
async fn worker_capacity_reduction_rechecks_usage_after_concurrent_reservation() -> TestResult {
    // Given: a two-slot worker with one assignment and a reducer paused after reading that usage.
    let fixture = fixture().await?;
    let (worker, second_task) = prepare_capacity_race(&fixture).await?;
    let second_task_id = second_task.id;
    let (usage_snapshot_tx, usage_snapshot_rx) = oneshot::channel();
    let (reservation_committed_tx, reservation_committed_rx) = oneshot::channel();
    let reducing_store = fixture.store.clone();
    let reserving_store = fixture.store.clone();
    let update = WorkerUpdate {
        id: worker.id,
        expected_version: worker.version,
        name: worker.name,
        api_url: worker.api_url,
        enabled: worker.enabled,
        compute_slots: ComputeSlots::try_from(1_u64)?,
        updated_at: fixture.now,
    };

    // When: another reservation commits between the reducer's usage snapshot and worker update.
    let reduction = async move {
        let used = reducing_store.worker_used_slots(update.id).await?;
        usage_snapshot_tx
            .send(used)
            .map_err(|_| std::io::Error::other("usage checkpoint receiver dropped"))?;
        reservation_committed_rx.await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(
            reducing_store.update_worker(&update).await?,
        )
    };
    let reservation = async move {
        let used = usage_snapshot_rx.await?;
        assert_eq!(used, 1);
        assert!(matches!(
            reserving_store
                .reserve_task(&Reservation {
                    task_id: second_task_id,
                    expected_task_version: second_task.version,
                    worker_id: worker.id,
                    attempt_id: AttemptId::random(),
                    submission_key: SubmissionKey::random(),
                    reserved_at: fixture.now,
                })
                .await?,
            ReservationOutcome::Reserved(_)
        ));
        reservation_committed_tx
            .send(())
            .map_err(|()| std::io::Error::other("reduction checkpoint receiver dropped"))?;
        TestResult::Ok(())
    };
    let (update_outcome, ()) = tokio::try_join!(reduction, reservation)?;

    // Then: durable assignments never exceed the worker's durable compute slots.
    assert_eq!(update_outcome, WorkerUpdateOutcome::CapacityBelowUsage);
    let worker = fixture
        .store
        .worker(worker.id)
        .await?
        .ok_or_else(|| std::io::Error::other("worker missing after race"))?;
    let used = fixture.store.worker_used_slots(worker.id).await?;
    assert!(used <= u64::from(worker.compute_slots.get()));
    Ok(())
}

async fn prepare_capacity_race(fixture: &Fixture) -> TestResult<(WorkerRecord, TaskRecord)> {
    let worker = fixture
        .registry
        .create(
            worker_request("worker-a", "https://worker.example/api/", 2)?,
            fixture.now,
        )
        .await?;
    online(fixture, worker.id, worker.version, &["anime-upscale"]).await?;
    let worker = fixture
        .registry
        .worker(worker.id)
        .await?
        .ok_or_else(|| std::io::Error::other("worker missing"))?;
    let first_task_id = task_id(403);
    let second_task_id = task_id(404);
    fixture
        .store
        .insert_task(&task(first_task_id, "anime-upscale", 10, fixture.now))
        .await?;
    fixture
        .store
        .insert_task(&task(second_task_id, "anime-upscale", 9, fixture.now))
        .await?;
    let first_task = fixture
        .store
        .task(first_task_id)
        .await?
        .ok_or_else(|| std::io::Error::other("first task missing"))?;
    assert!(matches!(
        fixture
            .store
            .reserve_task(&Reservation {
                task_id: first_task_id,
                expected_task_version: first_task.version,
                worker_id: worker.id,
                attempt_id: AttemptId::random(),
                submission_key: SubmissionKey::random(),
                reserved_at: fixture.now,
            })
            .await?,
        ReservationOutcome::Reserved(_)
    ));
    let second_task = fixture
        .store
        .task(second_task_id)
        .await?
        .ok_or_else(|| std::io::Error::other("second task missing"))?;
    Ok((worker, second_task))
}
