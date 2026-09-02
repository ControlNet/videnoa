use std::time::Duration;

use videnoa_controller::domain::TaskStatus;
use videnoa_controller::lifecycle::JitterSample;
use videnoa_controller::recovery::{
    Reconciler, RecoveryCommandKind, RecoveryConfig, ShutdownCoordinator,
};
use videnoa_controller::remote::{PayloadLimits, RemoteTimeouts};

use crate::mock_videnoa::faults::OfflineMode;
use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{output_path, verified_task};
use crate::transfer_support::{Fixture, TestResult};

#[tokio::test]
async fn startup_dispatch_converges_verified_publication_and_cleanup() -> TestResult {
    // Given: startup reconciliation observes a durable verified artifact.
    let server = MockVidenoa::start().await?;
    let output = b"startup publication".repeat(1024);
    let (fixture, prepared) = verified_task(&server, &output).await?;
    let reconciler = reconciler(&fixture)?;
    let report = reconciler.reconcile_startup(fixture.now).await?;
    assert_eq!(
        report.command_kind(prepared.task_id),
        Some(RecoveryCommandKind::Verify)
    );

    // When: the production recovery dispatcher executes the report.
    let advanced = fixture
        .executor()?
        .dispatch_recovery(&report, fixture.now, JitterSample::default())
        .await?;

    // Then: publication, local cleanup, and remote cleanup reach completed durably.
    assert_eq!(advanced, vec![prepared.task_id]);
    assert_eq!(
        fixture.task(prepared.task_id).await?.status,
        TaskStatus::Completed
    );
    assert_eq!(
        tokio::fs::read(output_path(&fixture, &prepared).await?).await?,
        output
    );
    Ok(())
}

#[tokio::test]
async fn startup_publishes_locally_while_remote_cleanup_is_offline() -> TestResult {
    // Given: a verified task restarts while its assigned worker returns service unavailable.
    let mut server = MockVidenoa::start().await?;
    let output = b"offline cleanup publication".repeat(1024);
    let (fixture, prepared) = verified_task(&server, &output).await?;
    server.set_offline(OfflineMode::ServiceUnavailable).await?;
    let reconciler = reconciler(&fixture)?;

    // When: startup reconciliation and dispatch run without worker health.
    let report = reconciler.reconcile_startup(fixture.now).await?;
    let advanced = fixture
        .executor()?
        .dispatch_recovery(&report, fixture.now, JitterSample::default())
        .await?;

    // Then: final output and local cleanup converge while remote cleanup is durably retried.
    assert_eq!(
        report.command_kind(prepared.task_id),
        Some(RecoveryCommandKind::Verify)
    );
    assert!(advanced.is_empty());
    let task = fixture.task(prepared.task_id).await?;
    assert_eq!(task.status, TaskStatus::RemoteCleanup);
    assert_eq!(task.retry.retry_count, 1);
    assert_eq!(
        tokio::fs::read(output_path(&fixture, &prepared).await?).await?,
        output
    );
    assert!(!fixture
        .temp_root
        .join(prepared.task_id.to_string())
        .exists());
    Ok(())
}

fn reconciler(fixture: &Fixture) -> TestResult<Reconciler> {
    Ok(Reconciler::new(
        fixture.store.clone(),
        RecoveryConfig::new(
            RemoteTimeouts::new(
                Duration::from_secs(1),
                Duration::from_secs(3),
                Duration::from_secs(1),
            )?,
            PayloadLimits::new(1024 * 1024, 4096)?,
            Duration::from_secs(1),
            Duration::from_secs(4),
            3,
        ),
        ShutdownCoordinator::new(),
    ))
}
