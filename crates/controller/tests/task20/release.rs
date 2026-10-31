//! Failed tasks release the disk they no longer need.

use std::path::Path;
use std::time::Duration;

use videnoa_controller::domain::{FailureCode, Task, TaskAction, TaskDetailResponse, TaskStatus};

use crate::mock_videnoa::checkpoints::Checkpoint;
use crate::mock_videnoa::domain::JobStatus;
use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{complete_mock_job, ControllerFixture, TestResult};

const FIRST_JOB_ID: &str = "00000000-0000-4000-8000-000000000001";

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn output_collision_releases_the_verified_output() -> TestResult {
    // Given: a file appears at the task's output path while the job runs.
    let worker = MockVidenoa::start_persistent().await?;
    let fixture = ControllerFixture::start().await?;
    fixture.register_worker(&worker, "collision").await?;
    let run = worker
        .pause(Checkpoint::AfterRunPersistedBeforeResponse)
        .await;
    let task = fixture.create_task("collision", b"input-video").await?;
    worker.await_checkpoint(&run).await?;
    let output = fixture.output_root.join("collision.mp4");
    tokio::fs::write(&output, b"user-file").await?;

    // When: the verified output cannot be published.
    complete_mock_job(&worker, &task, b"enhanced-video").await?;
    worker.release(run).await?;
    let failed = wait_for_status(&fixture, &task, TaskStatus::Failed).await?;

    // Then: retry is impossible, so the downloaded output is released; the
    // user's file is untouched.
    let failure = failed
        .task
        .failure
        .ok_or_else(|| std::io::Error::other("collision has no failure"))?;
    assert_eq!(failure.failure_code, FailureCode::OutputExists);
    assert_eq!(failed.task.allowed_actions, [TaskAction::Discard]);
    wait_for_absent(&fixture.temp_root.join(task.id.to_string())).await?;
    assert_eq!(tokio::fs::read(&output).await?, b"user-file");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn startup_releases_files_a_crash_left_behind() -> TestResult {
    // Given: a failed task that no retry can resume still has temporary files,
    // as after a crash between its failure and their release, next to files
    // of a task the Controller does not know.
    let worker = MockVidenoa::start_persistent().await?;
    let mut fixture = ControllerFixture::start().await?;
    fixture.register_worker(&worker, "leftover").await?;
    let run = worker
        .pause(Checkpoint::AfterRunPersistedBeforeResponse)
        .await;
    let task = fixture.create_task("leftover", b"input-video").await?;
    worker.await_checkpoint(&run).await?;
    tokio::fs::write(fixture.output_root.join("leftover.mp4"), b"user-file").await?;
    complete_mock_job(&worker, &task, b"enhanced-video").await?;
    worker.release(run).await?;
    wait_for_status(&fixture, &task, TaskStatus::Failed).await?;
    let workspace = fixture.temp_root.join(task.id.to_string());
    wait_for_absent(&workspace).await?;
    fixture.crash().await?;
    tokio::fs::create_dir(&workspace).await?;
    tokio::fs::write(workspace.join("output.mp4.verified"), b"enhanced-video").await?;
    let unknown = fixture.temp_root.join(uuid::Uuid::new_v4().to_string());
    tokio::fs::create_dir(&unknown).await?;

    // When: the Controller starts again.
    fixture.restart().await?;

    // Then: the failed task's files are released; unknown files are kept.
    wait_for_absent(&workspace).await?;
    assert!(tokio::fs::try_exists(&unknown).await?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn discarding_a_failed_task_releases_its_worker_folder_and_ends_retry() -> TestResult {
    // Given: a retryable processing failure that keeps its uploaded input.
    let worker = MockVidenoa::start_persistent().await?;
    let fixture = ControllerFixture::start().await?;
    fixture.register_worker(&worker, "discard").await?;
    let task = fixture.create_task("discard", b"input-video").await?;
    wait_for_status(&fixture, &task, TaskStatus::Processing).await?;
    worker
        .set_job(FIRST_JOB_ID, JobStatus::Failed, None)
        .await?;
    let failed = wait_for_status(&fixture, &task, TaskStatus::Failed).await?;
    assert_eq!(
        failed.task.failure.map(|failure| failure.failure_code),
        Some(FailureCode::ProcessingFailed)
    );
    assert_eq!(
        failed.task.allowed_actions,
        [
            TaskAction::Retry,
            TaskAction::RetryOnAnotherWorker,
            TaskAction::Discard
        ]
    );
    assert!(worker.file_count().await > 0);

    // When: the operator discards it.
    assert_eq!(
        fixture.discard_status(&task).await?,
        reqwest::StatusCode::OK
    );

    // Then: the Worker folder is gone, the task stays failed, and neither
    // retry nor a second discard is accepted.
    assert_eq!(worker.file_count().await, 0);
    let discarded = fixture.task(&task).await?;
    assert_eq!(discarded.task.status, TaskStatus::Failed);
    assert!(discarded.task.discarded_at.is_some());
    assert!(discarded.task.allowed_actions.is_empty());
    assert!(!fixture.temp_root.join(task.id.to_string()).exists());
    assert_eq!(
        fixture.retry_status(&task).await?,
        reqwest::StatusCode::CONFLICT
    );
    assert_eq!(
        fixture.discard_status(&task).await?,
        reqwest::StatusCode::CONFLICT
    );
    Ok(())
}

async fn wait_for_absent(path: &Path) -> TestResult {
    tokio::time::timeout(
        crate::mock_videnoa::deadline::eventually(Duration::from_secs(10)),
        async {
            while tokio::fs::try_exists(path).await? {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Ok::<_, std::io::Error>(())
        },
    )
    .await
    .map_err(|_| std::io::Error::other(format!("{} was not released", path.display())))??;
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
