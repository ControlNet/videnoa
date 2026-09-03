use std::time::Duration;

use videnoa_controller::domain::{FailureCode, TaskStatus};
use videnoa_controller::lifecycle::{
    AdvanceCommand, JitterSample, LifecycleService, PublicationIntent,
};
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

#[tokio::test]
async fn malformed_cleanup_does_not_abort_other_startup_work() -> TestResult {
    // Given: one cleanup task has incomplete worker evidence and another task is verified.
    let server = MockVidenoa::start().await?;
    let (fixture, malformed) = verified_task(&server, b"malformed cleanup").await?;
    let task = fixture.task(malformed.task_id).await?;
    let attempt = fixture.attempt(malformed.attempt_id).await?;
    LifecycleService::new(fixture.store.clone())
        .advance(
            &task,
            &attempt,
            AdvanceCommand::FinishVerification(PublicationIntent::new(
                ".videnoa-malformed-cleanup.staging",
            )),
            fixture.now,
        )
        .await?;
    let task = fixture.task(malformed.task_id).await?;
    let attempt = fixture.attempt(malformed.attempt_id).await?;
    LifecycleService::new(fixture.store.clone())
        .advance(
            &task,
            &attempt,
            AdvanceCommand::FinishPublication,
            fixture.now,
        )
        .await?;
    sqlx::query("UPDATE task_attempts SET worker_id = NULL WHERE id = ?")
        .bind(malformed.attempt_id.to_string())
        .execute(fixture.store.database().pool())
        .await?;
    let valid = fixture
        .remote_completed(&server, b"valid startup publication")
        .await?;
    let outcome = fixture
        .executor()?
        .download(valid.task_id, fixture.now, JitterSample::default())
        .await?;
    assert!(matches!(
        outcome,
        videnoa_controller::scheduler::DownloadOutcome::Verified(_)
    ));
    let reconciler = reconciler(&fixture)?;
    let report = reconciler.reconcile_startup(fixture.now).await?;

    // When: production startup dispatch consumes the whole report.
    let advanced = fixture
        .executor()?
        .dispatch_recovery(&report, fixture.now, JitterSample::default())
        .await?;

    // Then: valid work completes and malformed cleanup is durably terminalized.
    assert!(advanced.contains(&valid.task_id));
    assert_eq!(
        fixture.task(valid.task_id).await?.status,
        TaskStatus::Completed
    );
    let malformed_task = fixture.task(malformed.task_id).await?;
    assert_eq!(malformed_task.status, TaskStatus::Failed);
    assert_eq!(
        malformed_task.failure.map(|failure| failure.failure_code),
        Some(FailureCode::RemoteStateAmbiguous)
    );
    Ok(())
}

fn reconciler(fixture: &Fixture) -> TestResult<Reconciler> {
    Ok(Reconciler::new(
        fixture.store.clone(),
        RecoveryConfig::new(
            fixture.temp_root.clone(),
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
