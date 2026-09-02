use std::time::Duration;

use videnoa_controller::lifecycle::JitterSample;
use videnoa_controller::recovery::{
    Reconciler, RecoveryCommandKind, RecoveryConfig, ShutdownCoordinator,
};
use videnoa_controller::remote::{PayloadLimits, RemoteTimeouts};

use crate::mock_videnoa::journal::Route;
use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{Fixture, TestResult};

#[tokio::test]
async fn recovery_dispatch_executes_upload_through_production_executor() -> TestResult {
    // Given: startup reconciliation emits upload work for a durable reserved task.
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1, 1).await?;
    let prepared = fixture.reserved_task(vec![23_u8; 20_000]).await?;
    let timeouts = RemoteTimeouts::new(
        Duration::from_secs(1),
        Duration::from_secs(3),
        Duration::from_secs(1),
    )?;
    let limits = PayloadLimits::new(1024 * 1024, 4096)?;
    let reconciler = Reconciler::new(
        fixture.store.clone(),
        RecoveryConfig::new(
            timeouts,
            limits,
            Duration::from_secs(1),
            Duration::from_secs(4),
            3,
        ),
        ShutdownCoordinator::new(),
    );
    let report = reconciler.reconcile_startup(fixture.now).await?;
    assert_eq!(
        report.command_kind(prepared.task_id),
        Some(RecoveryCommandKind::Upload)
    );

    // When: the production recovery dispatcher consumes the durable report.
    let advanced = fixture
        .executor()?
        .dispatch_recovery(&report, fixture.now, JitterSample::default())
        .await?;
    let followup = reconciler
        .reconcile_task_id(prepared.task_id, fixture.now)
        .await?;

    // Then: a real PUT/stat advances into submission without a test-only direct transfer call.
    assert_eq!(advanced, vec![prepared.task_id]);
    assert_eq!(
        fixture.task(prepared.task_id).await?.status,
        videnoa_controller::domain::TaskStatus::Processing
    );
    assert_eq!(
        followup.command_kind(prepared.task_id),
        Some(RecoveryCommandKind::Poll)
    );
    assert_eq!(server.counters().await.get(Route::Upload), 1);
    assert_eq!(server.counters().await.get(Route::Run), 1);
    Ok(())
}

#[tokio::test]
async fn recovery_dispatch_executes_download_through_production_executor() -> TestResult {
    // Given: startup reconciliation emits download work for confirmed remote completion.
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1, 1).await?;
    let output = vec![61_u8; 20_000];
    let prepared = fixture.remote_completed(&server, &output).await?;
    let timeouts = RemoteTimeouts::new(
        Duration::from_secs(1),
        Duration::from_secs(3),
        Duration::from_secs(1),
    )?;
    let limits = PayloadLimits::new(1024 * 1024, 4096)?;
    let reconciler = Reconciler::new(
        fixture.store.clone(),
        RecoveryConfig::new(
            timeouts,
            limits,
            Duration::from_secs(1),
            Duration::from_secs(4),
            3,
        ),
        ShutdownCoordinator::new(),
    );
    let report = reconciler.reconcile_startup(fixture.now).await?;
    assert_eq!(
        report.command_kind(prepared.task_id),
        Some(RecoveryCommandKind::Download)
    );

    // When: the production recovery dispatcher consumes the durable report.
    let advanced = fixture
        .executor()?
        .dispatch_recovery(&report, fixture.now, JitterSample::default())
        .await?;

    // Then: the real stat/GET path advances the task to verification.
    assert_eq!(advanced, vec![prepared.task_id]);
    assert_eq!(
        fixture.task(prepared.task_id).await?.status,
        videnoa_controller::domain::TaskStatus::Verifying
    );
    assert_eq!(server.counters().await.get(Route::Download), 1);
    Ok(())
}
