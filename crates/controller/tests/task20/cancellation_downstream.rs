use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use sha2::{Digest, Sha256};
use videnoa_controller::domain::{Task, TaskDetailResponse, TaskStatus};
use videnoa_controller::lifecycle::{AdvanceCommand, DownloadEvidence, LifecycleService};
use videnoa_controller::persistence::Sha256Digest;
use videnoa_controller::scheduler::{TransferCheckpointObserver, TransferCheckpointPoint};

use crate::mock_videnoa::checkpoints::Checkpoint;
use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{complete_mock_job, CheckpointGate, ControllerFixture, TestResult};

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn downloading_cancellation_removes_positive_partial_workspace() -> TestResult {
    // Given: a real download has written positive bytes into the task-owned partial file.
    let worker = MockVidenoa::start_persistent().await?;
    let mut fixture = ControllerFixture::start().await?;
    fixture
        .register_worker(&worker, "cancel-downloading")
        .await?;
    let run = worker
        .pause(Checkpoint::AfterRunPersistedBeforeResponse)
        .await;
    let download = worker.pause(Checkpoint::MidDownloadBody).await;
    let task = fixture
        .create_task("cancel-downloading", b"input-video")
        .await?;
    worker.await_checkpoint(&run).await?;
    complete_mock_job(&worker, &task, b"enhanced-video").await?;
    worker.release(run).await?;
    worker.await_checkpoint(&download).await?;
    let workspace = fixture.temp_root.join(task.id.to_string());
    let part = workspace.join("output.mp4.part");
    assert_eq!(
        fixture.task(&task).await?.task.status,
        TaskStatus::Downloading
    );
    assert!(tokio::fs::metadata(&part).await?.len() > 0);

    // When: cancellation intent is persisted and the blocked Controller generation crashes.
    let task_record = fixture
        .store
        .task(task.id)
        .await?
        .ok_or_else(|| std::io::Error::other("downloading task record is missing"))?;
    let attempt = fixture
        .store
        .current_attempt(task.id)
        .await?
        .ok_or_else(|| std::io::Error::other("downloading attempt record is missing"))?;
    LifecycleService::new(fixture.store.clone())
        .request_cancellation(&task_record, Some(&attempt), Utc::now())
        .await?;
    fixture.crash().await?;
    worker.release(download).await?;
    fixture.restart().await?;
    wait_for_status(&fixture, &task, TaskStatus::Cancelled).await?;

    // Then: cancellation converges only after the complete local and remote workspaces are gone.
    assert!(!workspace.exists());
    assert_eq!(worker.file_count().await, 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn verifying_cancellation_removes_verified_workspace() -> TestResult {
    // Given: a real download installed verified bytes and durable evidence before its lifecycle CAS.
    let worker = MockVidenoa::start_persistent().await?;
    let gate = CheckpointGate::new(TransferCheckpointPoint::DownloadVerified);
    let observer: Arc<dyn TransferCheckpointObserver> = gate.clone();
    let mut fixture = ControllerFixture::start_with_checkpoint_observer(Some(observer)).await?;
    fixture.register_worker(&worker, "cancel-verifying").await?;
    let run = worker
        .pause(Checkpoint::AfterRunPersistedBeforeResponse)
        .await;
    let task = fixture
        .create_task("cancel-verifying", b"input-video")
        .await?;
    worker.await_checkpoint(&run).await?;
    complete_mock_job(&worker, &task, b"enhanced-video").await?;
    worker.release(run).await?;
    gate.wait().await?;
    let workspace = fixture.temp_root.join(task.id.to_string());
    assert!(workspace.join("output.mp4.verified").exists());
    assert!(workspace.join("output.mp4.verified.evidence").exists());
    fixture.crash().await?;
    gate.release();
    let task_record = fixture
        .store
        .task(task.id)
        .await?
        .ok_or_else(|| std::io::Error::other("download task record is missing"))?;
    let attempt = fixture
        .store
        .current_attempt(task.id)
        .await?
        .ok_or_else(|| std::io::Error::other("download attempt record is missing"))?;
    LifecycleService::new(fixture.store.clone())
        .advance(
            &task_record,
            &attempt,
            AdvanceCommand::FinishDownload(DownloadEvidence {
                size: 14,
                sha256: Sha256Digest::new(Sha256::digest(b"enhanced-video").into()),
            }),
            Utc::now(),
        )
        .await?;
    let task_record = fixture
        .store
        .task(task.id)
        .await?
        .ok_or_else(|| std::io::Error::other("verifying task record is missing"))?;
    let attempt = fixture
        .store
        .current_attempt(task.id)
        .await?
        .ok_or_else(|| std::io::Error::other("verifying attempt record is missing"))?;
    assert_eq!(task_record.status, TaskStatus::Verifying);
    LifecycleService::new(fixture.store.clone())
        .request_cancellation(&task_record, Some(&attempt), Utc::now())
        .await?;

    // When: a fresh Controller generation reconciles the durable Verifying cancellation.
    fixture.restart().await?;
    wait_for_status(&fixture, &task, TaskStatus::Cancelled).await?;

    // Then: verified bytes and their evidence are removed with the whole task workspace.
    assert!(!workspace.exists());
    assert_eq!(worker.file_count().await, 0);
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
    .map_err(|_| std::io::Error::other("task did not reach downstream cancellation status"))?
}
