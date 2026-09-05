use videnoa_controller::domain::{AttemptId, SubmissionKey};
use videnoa_controller::lifecycle::{LifecycleErrorCode, LifecycleService, ReserveCommand};
use videnoa_controller::persistence::SettingsUpdate;

use super::support::{fixture, online, task, task_id, worker_request, TestResult};

#[path = "atomic/capacity_reduction.rs"]
mod capacity_reduction;

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
