//! Failed tasks release the disk they no longer need.

use std::path::Path;
use std::time::Duration;

use videnoa_controller::domain::{FailureCode, Task, TaskDetailResponse, TaskStatus};

use crate::mock_videnoa::checkpoints::Checkpoint;
use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{complete_mock_job, ControllerFixture, TestResult};

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
