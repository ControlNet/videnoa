//! Tasks on a Worker that is gone for good: the operator abandons them and
//! then deletes the Worker.

use std::time::Duration;

use videnoa_controller::domain::{
    FailureCode, Task, TaskAction, TaskDetailResponse, TaskStatus, WorkerName,
};

use crate::mock_videnoa::checkpoints::Checkpoint;
use crate::mock_videnoa::faults::OfflineMode;
use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{ControllerFixture, TestResult};

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn abandoned_cancellation_on_a_dead_worker_frees_it_for_deletion() -> TestResult {
    // Given: a processing task whose Worker then refuses every connection.
    let mut worker = MockVidenoa::start_persistent().await?;
    let fixture = ControllerFixture::start().await?;
    let registered = fixture.register_worker(&worker, "dead-worker").await?;
    let run = worker
        .pause(Checkpoint::AfterRunPersistedBeforeResponse)
        .await;
    let task = fixture.create_task("dead-worker", b"input-video").await?;
    worker.await_checkpoint(&run).await?;
    worker.release(run).await?;
    let processing = wait_for_status(&fixture, &task, TaskStatus::Processing).await?;
    assert_eq!(processing.task.allowed_actions, [TaskAction::Cancel]);
    worker.set_offline(OfflineMode::ConnectionRefused).await?;

    // When: the cancel cannot reach the Worker, the operator abandons the task.
    assert_eq!(fixture.cancel_status(&task).await?, reqwest::StatusCode::OK);
    let waiting = fixture.task(&task).await?;
    assert_eq!(waiting.task.status, TaskStatus::Processing);
    assert!(waiting.task.cancel_requested_at.is_some());
    assert_eq!(waiting.task.allowed_actions, [TaskAction::Abandon]);
    assert_eq!(
        fixture.abandon_status(&task).await?,
        reqwest::StatusCode::OK
    );

    // Then: the task is terminal, says the Worker side is unverified, holds no
    // capacity or temporary files, and the Worker can be deleted.
    let abandoned = fixture.task(&task).await?;
    assert_eq!(abandoned.task.status, TaskStatus::Failed);
    assert_eq!(abandoned.task.allowed_actions, [TaskAction::Discard]);
    let failure = abandoned
        .task
        .failure
        .ok_or_else(|| std::io::Error::other("abandoned task has no failure"))?;
    assert_eq!(failure.failure_code, FailureCode::WorkerUnavailable);
    assert!(!failure.retryable);
    assert!(
        failure.message.contains("not confirmed"),
        "{}",
        failure.message
    );
    let capacity = fixture.store.worker_capacity(registered.id).await?;
    assert_eq!(capacity.used_slots, 0);
    assert_eq!(capacity.assigned_tasks, 0);
    assert!(!fixture.temp_root.join(task.id.to_string()).exists());
    assert_eq!(
        fixture.delete_worker_status(registered.id).await?,
        reqwest::StatusCode::OK
    );

    // And: history still names the deleted Worker.
    let history = fixture.task(&task).await?;
    assert_eq!(history.task.worker_id, None);
    let [attempt] = history.attempts.as_slice() else {
        return Err(std::io::Error::other("abandoned task lost its attempt").into());
    };
    assert_eq!(attempt.worker_id, None);
    assert_eq!(
        attempt.worker_name.as_ref().map(WorkerName::as_str),
        Some("dead-worker")
    );
    Ok(())
}

async fn wait_for_status(
    fixture: &ControllerFixture,
    task: &Task,
    status: TaskStatus,
) -> TestResult<TaskDetailResponse> {
    tokio::time::timeout(
        crate::mock_videnoa::deadline::eventually(Duration::from_secs(10)),
        async {
            loop {
                let detail = fixture.task(task).await?;
                if detail.task.status == status {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(detail);
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        },
    )
    .await
    .map_err(|_| std::io::Error::other(format!("task did not reach {status:?}")))?
}
