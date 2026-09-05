use std::time::Duration;

use chrono::Utc;
use videnoa_controller::recovery::{DrainOutcome, ShutdownCoordinator, ShutdownError};
use videnoa_controller::scheduler::Scheduler;

use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{ControllerFixture, TestResult};

#[tokio::test]
async fn shutdown_drain_waits_for_in_flight_stage_lifetime() {
    let coordinator = ShutdownCoordinator::new();
    let stage = coordinator.begin_stage().expect("stage intake is open");
    coordinator.stop_stage_intake();

    assert_eq!(
        coordinator.drain(Duration::from_millis(10)).await,
        DrainOutcome::TimedOut {
            outstanding_writes: 0
        }
    );
    assert_eq!(coordinator.outstanding_stages(), 1);

    drop(stage);
    assert_eq!(
        coordinator.drain(Duration::from_secs(1)).await,
        DrainOutcome::Drained
    );
}

#[tokio::test]
async fn coordinated_shutdown_rejects_an_incomplete_drain() -> TestResult {
    // Given: a live stage remains admitted when coordinated shutdown begins.
    let fixture = ControllerFixture::start().await?;
    let scheduler = Scheduler::load(fixture.store.clone())?;
    let coordinator = ShutdownCoordinator::new();
    let _stage = coordinator
        .begin_stage()
        .ok_or_else(|| std::io::Error::other("stage intake is closed"))?;

    // When: the bounded drain expires immediately.
    let error = coordinator
        .shutdown(&scheduler, Utc::now(), Duration::ZERO)
        .await
        .expect_err("incomplete drain must fail shutdown");

    // Then: the process boundary receives the outstanding stage and write counts.
    assert!(matches!(
        error,
        ShutdownError::DrainTimedOut {
            outstanding_stages: 1,
            outstanding_writes: 0
        }
    ));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn fatal_stage_persistence_failure_terminates_orchestration() -> TestResult {
    let worker = MockVidenoa::start_persistent().await?;
    let mut fixture = ControllerFixture::start().await?;
    fixture
        .register_worker(&worker, "fatal-stage-error")
        .await?;
    sqlx::query(
        "CREATE TRIGGER task20_fail_upload BEFORE UPDATE OF status ON tasks
         WHEN NEW.status = 'uploading'
         BEGIN SELECT RAISE(ABORT, 'task20 upload persistence failure'); END",
    )
    .execute(fixture.store.database().pool())
    .await?;

    fixture
        .create_task("fatal-stage-error", b"input-video")
        .await?;
    let error = fixture.wait_for_orchestration_error().await?;

    assert!(matches!(
        error,
        videnoa_controller::orchestration::OrchestrationError::Transfer(
            videnoa_controller::scheduler::TransferError::Lifecycle(_)
        )
    ));
    Ok(())
}
