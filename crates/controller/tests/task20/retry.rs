use std::time::Duration;

use videnoa_controller::domain::{FailureCode, Task, TaskDetailResponse, TaskStatus};

use crate::mock_videnoa::faults::{Fault, RestartMode, RestartOutcome};
use crate::mock_videnoa::journal::Route;
use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{ControllerFixture, TestResult};

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn explicit_processing_retry_creates_replacement_attempt_and_converges() -> TestResult {
    let mut worker = MockVidenoa::start_persistent().await?;
    worker.set_fault(Fault::AcceptThenDropRunResponse).await;
    let fixture = ControllerFixture::start().await?;
    let registered = fixture.register_worker(&worker, "worker-retry").await?;
    let task = fixture.create_task("worker-retry", b"input-video").await?;
    wait_for_job_count(&worker, 1).await?;
    assert_eq!(
        worker.restart(RestartMode::RetainState).await?,
        RestartOutcome::Retained
    );
    let failed = wait_for_status(&fixture, &task, TaskStatus::Failed).await?;
    assert_eq!(
        failed
            .task
            .failure
            .as_ref()
            .map(|failure| failure.failure_code),
        Some(FailureCode::ProcessingFailed)
    );
    let original = failed
        .attempts
        .first()
        .ok_or_else(|| std::io::Error::other("failed attempt missing"))?;

    let retried = fixture.retry_task(&task).await?;
    let processing = wait_for_status(&fixture, &task, TaskStatus::Processing).await?;
    let replacement = processing
        .attempts
        .iter()
        .find(|attempt| attempt.id == retried.attempt_id)
        .ok_or_else(|| std::io::Error::other("replacement attempt missing"))?;
    assert_eq!(retried.attempt_id, replacement.id);
    assert_ne!(replacement.id, original.id);
    assert_ne!(replacement.submission_key, original.submission_key);
    assert_eq!(processing.task.attempt_count, 2);
    let remote_job_id = replacement
        .remote_job_id
        .ok_or_else(|| std::io::Error::other("replacement remote job missing"))?;
    worker
        .complete_job(
            &remote_job_id.to_string(),
            &format!("{}/output.mp4", task.id),
            b"enhanced-video",
        )
        .await?;

    let completed = wait_for_status(&fixture, &task, TaskStatus::Completed).await?;
    assert_eq!(completed.attempts.len(), 2);
    assert_eq!(completed.task.attempt_count, 2);
    assert_eq!(worker.counters().await.get(Route::Run), 2);
    assert_eq!(worker.file_count().await, 0);
    assert_eq!(fixture.store.worker_used_slots(registered.id).await?, 0);
    assert_eq!(
        tokio::fs::read(completed.task.output_path.as_str()).await?,
        b"enhanced-video"
    );
    Ok(())
}

async fn wait_for_job_count(worker: &MockVidenoa, expected: usize) -> TestResult {
    tokio::time::timeout(Duration::from_secs(10), async {
        while worker.job_count().await != expected {
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| std::io::Error::other("remote job count did not converge"))?;
    Ok(())
}

async fn wait_for_status(
    fixture: &ControllerFixture,
    task: &Task,
    status: TaskStatus,
) -> TestResult<TaskDetailResponse> {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let detail = fixture.task(task).await?;
            if detail.task.status == status {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(detail);
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| std::io::Error::other("task did not reach retry status"))?
}
