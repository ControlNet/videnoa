use tokio::sync::oneshot;
use videnoa_controller::domain::{AttemptId, ComputeSlots, SubmissionKey, TaskId};
use videnoa_controller::lifecycle::{AdvanceCommand, LifecycleService};
use videnoa_controller::persistence::{
    AttemptRecord, Reservation, ReservationOutcome, TaskRecord, WorkerRecord, WorkerUpdate,
    WorkerUpdateOutcome,
};

use super::super::support::{fixture, online, task, task_id, worker_request, Fixture, TestResult};

#[tokio::test]
async fn worker_capacity_reduction_rechecks_usage_after_concurrent_compute_claim() -> TestResult {
    // Given: a two-slot worker with one active compute claim and one staged task.
    let fixture = fixture().await?;
    let (worker, second_task, second_attempt) = prepare_capacity_race(&fixture).await?;
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
    let submission = async move {
        let used = usage_snapshot_rx.await?;
        assert_eq!(used, 1);
        LifecycleService::new(reserving_store)
            .advance(
                &second_task,
                &second_attempt,
                AdvanceCommand::StartSubmission,
                fixture.now,
            )
            .await?;
        reservation_committed_tx
            .send(())
            .map_err(|()| std::io::Error::other("reduction checkpoint receiver dropped"))?;
        TestResult::Ok(())
    };
    let (update_outcome, ()) = tokio::try_join!(reduction, submission)?;

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

async fn prepare_capacity_race(
    fixture: &Fixture,
) -> TestResult<(WorkerRecord, TaskRecord, AttemptRecord)> {
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
    set_status(fixture, first_task_id, "staged").await?;
    let first_task = fixture
        .store
        .task(first_task_id)
        .await?
        .ok_or_else(|| std::io::Error::other("first staged task missing"))?;
    let first_attempt = fixture
        .store
        .current_attempt(first_task_id)
        .await?
        .ok_or_else(|| std::io::Error::other("first staged attempt missing"))?;
    LifecycleService::new(fixture.store.clone())
        .advance(
            &first_task,
            &first_attempt,
            AdvanceCommand::StartSubmission,
            fixture.now,
        )
        .await?;
    assert!(matches!(
        fixture
            .store
            .reserve_task(&Reservation {
                task_id: second_task_id,
                expected_task_version: 0,
                worker_id: worker.id,
                attempt_id: AttemptId::random(),
                submission_key: SubmissionKey::random(),
                reserved_at: fixture.now,
            })
            .await?,
        ReservationOutcome::Reserved(_)
    ));
    set_status(fixture, second_task_id, "staged").await?;
    let second_task = fixture
        .store
        .task(second_task_id)
        .await?
        .ok_or_else(|| std::io::Error::other("second task missing"))?;
    let second_attempt = fixture
        .store
        .current_attempt(second_task_id)
        .await?
        .ok_or_else(|| std::io::Error::other("second attempt missing"))?;
    Ok((worker, second_task, second_attempt))
}

async fn set_status(fixture: &Fixture, task_id: TaskId, status: &str) -> TestResult {
    let mut transaction = fixture.store.database().pool().begin().await?;
    sqlx::query("UPDATE tasks SET status = ?, version = version + 1 WHERE id = ?")
        .bind(status)
        .bind(task_id.to_string())
        .execute(&mut *transaction)
        .await?;
    sqlx::query("UPDATE task_attempts SET status = ?, version = version + 1 WHERE task_id = ?")
        .bind(status)
        .bind(task_id.to_string())
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(())
}
