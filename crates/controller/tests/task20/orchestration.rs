use videnoa_controller::domain::TaskStatus;
use videnoa_controller::scheduler::TransferCheckpointPoint;

use crate::mock_videnoa::checkpoints::Checkpoint;
use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{CheckpointGate, ControllerFixture, TestResult};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn active_first_page_cannot_starve_later_durable_work() -> TestResult {
    // Given: one older active task blocks at upload completion and fills the scan's first page.
    let first_worker = MockVidenoa::start_persistent().await?;
    let first_gate = CheckpointGate::new(TransferCheckpointPoint::BeforeRemoteSubmit);
    let fixture = ControllerFixture::start_with_recovery_page_size_and_checkpoint(
        std::num::NonZeroU16::MIN,
        first_gate.clone(),
    )
    .await?;
    let first_registered = fixture
        .register_worker(&first_worker, "page-one-active")
        .await?;
    let first_task = fixture
        .create_task("page-one-active", b"first-input")
        .await?;
    first_gate.wait().await?;

    let second_worker = MockVidenoa::start_persistent().await?;
    let second_boundary = second_worker.pause(Checkpoint::BeforeRunPersistence).await;
    let second_registered = fixture
        .register_worker(&second_worker, "later-eligible")
        .await?;

    // When: a later task is reserved while recurring orchestration scans one row per page.
    let second_task = fixture
        .create_task("later-eligible", b"second-input")
        .await?;
    second_worker.await_checkpoint(&second_boundary).await?;

    // Then: the later task is dispatched without repeating or stealing the active task's ownership.
    let first = fixture
        .store
        .task(first_task.id)
        .await?
        .ok_or_else(|| std::io::Error::other("first task missing"))?;
    let second = fixture
        .store
        .task(second_task.id)
        .await?
        .ok_or_else(|| std::io::Error::other("second task missing"))?;
    assert_eq!(first.status, TaskStatus::Staged);
    assert_eq!(first.worker_id, Some(first_registered.id));
    assert_eq!(second.status, TaskStatus::Submitting);
    assert_eq!(second.worker_id, Some(second_registered.id));

    second_worker.release(second_boundary).await?;
    first_gate.release();
    fixture.stop().await
}
