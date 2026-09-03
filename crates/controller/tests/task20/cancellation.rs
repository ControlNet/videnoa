use std::time::Duration;

use videnoa_controller::domain::{FailureCode, FailureStage, Task, TaskDetailResponse, TaskStatus};

use crate::mock_videnoa::checkpoints::Checkpoint;
use crate::mock_videnoa::faults::{DeleteOutcome, Fault};
use crate::mock_videnoa::journal::Route;
use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{ControllerFixture, TestResult};

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn accepted_processing_cancellation_converges_without_compute_replay() -> TestResult {
    let worker = MockVidenoa::start_persistent().await?;
    let fixture = ControllerFixture::start().await?;
    let registered = fixture.register_worker(&worker, "accepted-cancel").await?;
    let run = worker
        .pause(Checkpoint::AfterRunPersistedBeforeResponse)
        .await;
    let task = fixture
        .create_task("accepted-cancel", b"input-video")
        .await?;
    worker.await_checkpoint(&run).await?;
    worker.release(run).await?;
    wait_for_status(&fixture, &task, TaskStatus::Processing).await?;

    assert_eq!(fixture.cancel_status(&task).await?, reqwest::StatusCode::OK);
    let cancelled = wait_for_status(&fixture, &task, TaskStatus::Cancelled).await?;

    let [attempt] = cancelled.attempts.as_slice() else {
        return Err(
            std::io::Error::other("cancellation did not retain exactly one attempt").into(),
        );
    };
    assert_eq!(attempt.status, TaskStatus::Cancelled);
    assert_eq!(cancelled.task.attempt_count, 1);
    let capacity = fixture.store.worker_capacity(registered.id).await?;
    assert_eq!(capacity.used_slots, 0);
    assert_eq!(capacity.assigned_tasks, 0);
    assert_eq!(capacity.processing_tasks, 0);
    assert_eq!(capacity.active_uploads, 0);
    assert_eq!(capacity.active_downloads, 0);
    let counters = worker.counters().await;
    assert_eq!(counters.get(Route::Run), 1);
    assert_eq!(counters.get(Route::JobCancel), 1);
    assert_eq!(worker.job_count().await, 0);
    assert_eq!(worker.file_count().await, 0);
    assert!(!fixture.temp_root.join(task.id.to_string()).exists());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn rejected_cancellation_cleanup_persists_task_local_failure() -> TestResult {
    let worker = MockVidenoa::start_persistent().await?;
    let fixture = ControllerFixture::start().await?;
    fixture
        .register_worker(&worker, "rejected-cancel-cleanup")
        .await?;
    let run = worker
        .pause(Checkpoint::AfterRunPersistedBeforeResponse)
        .await;
    let task = fixture
        .create_task("rejected-cancel-cleanup", b"input-video")
        .await?;
    worker.await_checkpoint(&run).await?;
    worker.release(run).await?;
    wait_for_status(&fixture, &task, TaskStatus::Processing).await?;
    worker
        .set_fault(Fault::DeleteScript(vec![DeleteOutcome::ClientError]))
        .await;

    assert_eq!(fixture.cancel_status(&task).await?, reqwest::StatusCode::OK);
    let failed = wait_for_status(&fixture, &task, TaskStatus::Failed).await?;

    let failure = failed
        .task
        .failure
        .ok_or_else(|| std::io::Error::other("cleanup rejection did not retain failure proof"))?;
    assert_eq!(failure.failure_stage, FailureStage::RemoteCleanup);
    assert_eq!(failure.failure_code, FailureCode::CleanupFailed);
    assert!(!failure.retryable);
    assert_eq!(worker.counters().await.get(Route::DeleteFile), 1);
    assert_eq!(worker.counters().await.get(Route::Run), 1);
    Ok(())
}

async fn wait_for_status(
    fixture: &ControllerFixture,
    task: &Task,
    status: TaskStatus,
) -> TestResult<TaskDetailResponse> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let detail = fixture.task(task).await?;
            if detail.task.status == status {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(detail);
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| std::io::Error::other("task did not reach cancellation status"))?
}
