use std::sync::Arc;
use std::time::Duration;

use videnoa_controller::domain::TaskStatus;
use videnoa_controller::scheduler::{TransferCheckpointObserver, TransferCheckpointPoint};

use crate::mock_videnoa::checkpoints::Checkpoint;
use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{
    assert_completed_pipeline, complete_mock_job, CheckpointGate, ControllerFixture, TestResult,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn persisted_pause_blocks_submit_after_staging_then_resumes_same_attempt() -> TestResult {
    let worker = MockVidenoa::start_persistent().await?;
    let submit_gate = CheckpointGate::new(TransferCheckpointPoint::BeforeRemoteSubmit);
    let observer: Arc<dyn TransferCheckpointObserver> = submit_gate.clone();
    let fixture = ControllerFixture::start_with_checkpoint_observer(Some(observer)).await?;
    fixture
        .register_worker(&worker, "pause-before-submit")
        .await?;
    let run_ticket = worker.pause(Checkpoint::BeforeRunPersistence).await;

    let task = fixture
        .create_task("pause-before-submit", b"input-video")
        .await?;
    submit_gate.wait().await?;
    fixture.pause_scheduler().await?;
    submit_gate.release();

    let premature_run = tokio::time::timeout(
        Duration::from_millis(500),
        worker.await_checkpoint(&run_ticket),
    )
    .await;
    if let Ok(result) = premature_run {
        result?;
        worker.release(run_ticket).await?;
        return Err(std::io::Error::other("paused scheduler submitted remote work").into());
    }
    assert_eq!(
        fixture.task(&task).await?.task.status,
        TaskStatus::Submitting
    );

    fixture.resume_scheduler().await?;
    worker.await_checkpoint(&run_ticket).await?;
    worker.release(run_ticket).await?;
    complete_mock_job(&worker, &task, b"enhanced-video").await?;
    assert_completed_pipeline(&fixture, &worker, &task, b"enhanced-video").await
}
