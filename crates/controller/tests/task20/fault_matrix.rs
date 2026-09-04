use std::sync::Arc;

use videnoa_controller::domain::TaskStatus;
use videnoa_controller::scheduler::{TransferCheckpointObserver, TransferCheckpointPoint};

use crate::mock_videnoa::checkpoints::Checkpoint;
use crate::mock_videnoa::journal::Route;
use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{
    assert_restarted_pipeline, complete_mock_job, wait_for_positive_download_partial,
    CheckpointGate, ControllerFixture, TestResult,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn controller_restart_matrix_executes_every_remote_boundary() -> TestResult {
    eprintln!("task20 crash boundary: before reservation");
    restart_before_reservation().await?;
    eprintln!("task20 crash boundary: after reservation");
    restart_at_remote_checkpoint(Checkpoint::BeforeAcceptingUpload, TaskStatus::Uploading).await?;
    eprintln!("task20 crash boundary: mid upload");
    crate::fault_matrix_upload::restart_mid_upload().await?;
    eprintln!("task20 crash boundary: after upload");
    restart_after_upload().await?;
    eprintln!("task20 crash boundary: before submit");
    restart_before_submit().await?;
    eprintln!("task20 crash boundary: accepted submit before local persistence");
    restart_after_remote_acceptance().await?;
    eprintln!("task20 crash boundary: during poll");
    restart_during_poll().await?;
    eprintln!("task20 crash boundary: after remote completion");
    restart_after_remote_completion().await?;
    eprintln!("task20 crash boundary: mid download");
    restart_mid_download().await?;
    Ok(())
}

async fn restart_before_reservation() -> TestResult {
    let worker = MockVidenoa::start_persistent().await?;
    let mut fixture = ControllerFixture::start().await?;
    let task = fixture
        .create_task("before-reservation", b"input-video")
        .await?;
    assert_eq!(fixture.task(&task).await?.task.status, TaskStatus::Queued);
    fixture.crash().await?;
    fixture.restart().await?;
    let run = worker
        .pause(Checkpoint::AfterRunPersistedBeforeResponse)
        .await;
    fixture.register_worker(&worker, "worker-before").await?;
    worker.await_checkpoint(&run).await?;
    complete_mock_job(&worker, &task, b"enhanced-video").await?;
    worker.release(run).await?;
    assert_restarted_pipeline(&fixture, &worker, &task, b"enhanced-video").await
}

async fn restart_at_remote_checkpoint(
    checkpoint: Checkpoint,
    expected_status: TaskStatus,
) -> TestResult {
    let worker = MockVidenoa::start_persistent().await?;
    let mut fixture = ControllerFixture::start().await?;
    fixture.register_worker(&worker, checkpoint.name()).await?;
    let boundary = worker.pause(checkpoint).await;
    let task = fixture
        .create_task(checkpoint.name(), b"input-video")
        .await?;
    worker.await_checkpoint(&boundary).await?;
    assert_eq!(fixture.task(&task).await?.task.status, expected_status);
    fixture.crash().await?;
    worker.release(boundary).await?;
    fixture.restart().await?;
    wait_for_remote_job(&worker).await?;
    complete_mock_job(&worker, &task, b"enhanced-video").await?;
    assert_restarted_pipeline(&fixture, &worker, &task, b"enhanced-video").await
}

async fn restart_after_remote_acceptance() -> TestResult {
    let worker = MockVidenoa::start_persistent().await?;
    let mut fixture = ControllerFixture::start().await?;
    fixture.register_worker(&worker, "accepted-submit").await?;
    let run = worker
        .pause(Checkpoint::AfterRunPersistedBeforeResponse)
        .await;
    let task = fixture
        .create_task("accepted-submit", b"input-video")
        .await?;
    worker.await_checkpoint(&run).await?;
    assert_eq!(worker.job_count().await, 1);
    assert_eq!(
        fixture.task(&task).await?.task.status,
        TaskStatus::Submitting
    );
    fixture.crash().await?;
    complete_mock_job(&worker, &task, b"enhanced-video").await?;
    worker.release(run).await?;
    fixture.restart().await?;
    assert_restarted_pipeline(&fixture, &worker, &task, b"enhanced-video").await
}

async fn restart_after_upload() -> TestResult {
    let worker = MockVidenoa::start_persistent().await?;
    let gate = CheckpointGate::new(TransferCheckpointPoint::UploadCompleted);
    let observer: Arc<dyn TransferCheckpointObserver> = gate.clone();
    let mut fixture = ControllerFixture::start_with_checkpoint_observer(Some(observer)).await?;
    fixture.register_worker(&worker, "after-upload").await?;
    let task = fixture.create_task("after-upload", b"input-video").await?;
    gate.wait().await?;
    let detail = fixture.task(&task).await?;
    assert_eq!(detail.task.status, TaskStatus::Staged);
    assert_eq!(detail.attempts.len(), 1);
    assert_eq!(detail.attempts[0].status, TaskStatus::Staged);
    let counters = worker.counters().await;
    assert_eq!(counters.get(Route::Upload), 1);
    assert_eq!(counters.get(Route::Run), 0);
    fixture.crash().await?;
    gate.release();
    fixture.restart().await?;
    wait_for_remote_job(&worker).await?;
    complete_mock_job(&worker, &task, b"enhanced-video").await?;
    assert_restarted_pipeline(&fixture, &worker, &task, b"enhanced-video").await
}

async fn restart_before_submit() -> TestResult {
    let worker = MockVidenoa::start_persistent().await?;
    let gate = CheckpointGate::new(TransferCheckpointPoint::BeforeRemoteSubmit);
    let observer: Arc<dyn TransferCheckpointObserver> = gate.clone();
    let mut fixture = ControllerFixture::start_with_checkpoint_observer(Some(observer)).await?;
    fixture.register_worker(&worker, "before-submit").await?;
    let task = fixture.create_task("before-submit", b"input-video").await?;
    gate.wait().await?;
    let detail = fixture.task(&task).await?;
    assert_eq!(detail.task.status, TaskStatus::Submitting);
    assert_eq!(detail.attempts.len(), 1);
    assert_eq!(detail.attempts[0].status, TaskStatus::Submitting);
    let counters = worker.counters().await;
    assert_eq!(counters.get(Route::Upload), 1);
    assert_eq!(counters.get(Route::Run), 0);
    fixture.crash().await?;
    gate.release();
    fixture.restart().await?;
    wait_for_remote_job(&worker).await?;
    complete_mock_job(&worker, &task, b"enhanced-video").await?;
    assert_restarted_pipeline(&fixture, &worker, &task, b"enhanced-video").await
}

async fn restart_during_poll() -> TestResult {
    let worker = MockVidenoa::start_persistent().await?;
    let mut fixture = ControllerFixture::start().await?;
    fixture.register_worker(&worker, "during-poll").await?;
    let run = worker
        .pause(Checkpoint::AfterRunPersistedBeforeResponse)
        .await;
    let poll = worker.pause(Checkpoint::BeforePollResponse).await;
    let task = fixture.create_task("during-poll", b"input-video").await?;
    worker.await_checkpoint(&run).await?;
    worker.release(run).await?;
    worker.await_checkpoint(&poll).await?;
    assert_eq!(
        fixture.task(&task).await?.task.status,
        TaskStatus::Processing
    );
    fixture.crash().await?;
    complete_mock_job(&worker, &task, b"enhanced-video").await?;
    worker.release(poll).await?;
    fixture.restart().await?;
    assert_restarted_pipeline(&fixture, &worker, &task, b"enhanced-video").await
}

async fn restart_after_remote_completion() -> TestResult {
    let worker = MockVidenoa::start_persistent().await?;
    let gate = CheckpointGate::new(TransferCheckpointPoint::RemoteCompletionPersisted);
    let observer: Arc<dyn TransferCheckpointObserver> = gate.clone();
    let mut fixture = ControllerFixture::start_with_checkpoint_observer(Some(observer)).await?;
    fixture
        .register_worker(&worker, "after-remote-completion")
        .await?;
    let run = worker
        .pause(Checkpoint::AfterRunPersistedBeforeResponse)
        .await;
    let task = fixture
        .create_task("after-remote-completion", b"input-video")
        .await?;
    worker.await_checkpoint(&run).await?;
    complete_mock_job(&worker, &task, b"enhanced-video").await?;
    worker.release(run).await?;
    gate.wait().await?;
    let detail = fixture.task(&task).await?;
    assert_eq!(detail.task.status, TaskStatus::RemoteCompleted);
    assert_eq!(detail.attempts.len(), 1);
    assert_eq!(detail.attempts[0].status, TaskStatus::RemoteCompleted);
    let counters = worker.counters().await;
    assert!(counters.get(Route::JobPoll) >= 1);
    assert_eq!(counters.get(Route::Download), 0);
    fixture.crash().await?;
    gate.release();
    fixture.restart().await?;
    assert_restarted_pipeline(&fixture, &worker, &task, b"enhanced-video").await
}

async fn restart_mid_download() -> TestResult {
    let worker = MockVidenoa::start_persistent().await?;
    let mut fixture = ControllerFixture::start().await?;
    fixture.register_worker(&worker, "mid-download").await?;
    let run = worker
        .pause(Checkpoint::AfterRunPersistedBeforeResponse)
        .await;
    let download = worker.pause(Checkpoint::MidDownloadBody).await;
    let task = fixture.create_task("mid-download", b"input-video").await?;
    worker.await_checkpoint(&run).await?;
    complete_mock_job(&worker, &task, b"enhanced-video").await?;
    worker.release(run).await?;
    worker.await_checkpoint(&download).await?;
    let detail = fixture.task(&task).await?;
    assert_eq!(detail.task.status, TaskStatus::Downloading);
    assert_eq!(detail.attempts.len(), 1);
    assert_eq!(detail.attempts[0].status, TaskStatus::Downloading);
    assert_eq!(worker.counters().await.get(Route::Download), 1);
    wait_for_positive_download_partial(&fixture, &worker, &task).await?;
    fixture.crash().await?;
    worker.release(download).await?;
    fixture.restart().await?;
    assert_restarted_pipeline(&fixture, &worker, &task, b"enhanced-video").await
}

async fn wait_for_remote_job(worker: &MockVidenoa) -> TestResult {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if worker.job_count().await == 1 {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| std::io::Error::other("remote job was not created after restart"))?;
    Ok(())
}
