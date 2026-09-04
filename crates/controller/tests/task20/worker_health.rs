use std::future::Future;
use std::time::Duration;

use videnoa_controller::domain::{TaskStatus, WorkerId, WorkflowName};

use crate::mock_videnoa::faults::{Fault, ResponseFault};
use crate::mock_videnoa::journal::Route;
use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{ControllerFixture, TestResult};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn api_registered_worker_refresh_wakes_queued_scheduling() -> TestResult {
    // Given: a queued API task and a healthy worker registered only through Controller HTTP.
    let worker = MockVidenoa::start_persistent().await?;
    let fixture = ControllerFixture::start().await?;
    let task = fixture.create_task("health-wake", b"input-video").await?;
    let registered = fixture.register_worker(&worker, "health-wake").await?;

    // When: the production health runtime discovers the worker.
    let durable = wait_for_worker(&fixture, registered.id, |record| record.online).await?;
    let assigned = wait_for_task(&fixture, &task, |status| status != TaskStatus::Queued).await?;

    // Then: compatible capability evidence is durable and scheduling creates an attempt.
    assert!(durable
        .capabilities
        .workflows
        .iter()
        .any(|workflow| { workflow.name == WorkflowName::new("eligible-workflow.json") }));
    assert_eq!(assigned.attempts.len(), 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn failed_probe_persists_backoff_and_recovers_without_losing_capabilities() -> TestResult {
    // Given: an online compatible worker whose next health response fails once.
    let worker = MockVidenoa::start().await?;
    let fixture = ControllerFixture::start().await?;
    let registered = fixture.register_worker(&worker, "health-retry").await?;
    let initial = wait_for_worker(&fixture, registered.id, |record| record.online).await?;
    worker
        .set_fault(Fault::Response(ResponseFault {
            route: Route::Health,
            status: 503,
            body: Vec::new(),
        }))
        .await;

    // When: the periodic probe observes the transient outage.
    let failed = wait_for_worker(&fixture, registered.id, |record| {
        !record.online && record.health_retry_count == 1
    })
    .await?;

    // Then: retry state is bounded, useful durable evidence remains, and recovery clears failure.
    assert_eq!(failed.capabilities, initial.capabilities);
    assert!(failed.next_health_check_at.is_some());
    assert_eq!(
        failed.last_error.as_deref(),
        Some("worker health check failed")
    );
    let recovered = wait_for_worker(&fixture, registered.id, |record| {
        record.online && record.health_retry_count == 0 && record.last_error.is_none()
    })
    .await?;
    assert_eq!(
        recovered.capabilities.workflows,
        initial.capabilities.workflows
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn disabled_worker_is_not_probed_or_scheduled() -> TestResult {
    // Given: a healthy remote registered disabled and a compatible queued task.
    let worker = MockVidenoa::start().await?;
    let fixture = ControllerFixture::start().await?;
    let registered = fixture
        .register_worker_enabled(&worker, "health-disabled", false)
        .await?;
    let task = fixture
        .create_task("health-disabled", b"input-video")
        .await?;

    // When: more than one refresh cadence elapses.
    tokio::time::sleep(Duration::from_millis(1_200)).await;

    // Then: disabled policy prevents both remote observation and reservation.
    let durable = fixture
        .store
        .worker(registered.id)
        .await?
        .ok_or_else(|| std::io::Error::other("worker missing"))?;
    assert!(!durable.online);
    assert_eq!(worker.counters().await.get(Route::Health), 0);
    assert_eq!(fixture.task(&task).await?.task.status, TaskStatus::Queued);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn expired_capability_cache_replaces_durable_catalog() -> TestResult {
    // Given: an online worker with the normal compatible catalog.
    let worker = MockVidenoa::start().await?;
    let fixture = ControllerFixture::start().await?;
    let registered = fixture
        .register_worker(&worker, "health-capability")
        .await?;
    wait_for_worker(&fixture, registered.id, |record| record.online).await?;
    worker
        .set_fault(Fault::Response(ResponseFault {
            route: Route::Workflows,
            status: 200,
            body: b"[]".to_vec(),
        }))
        .await;
    worker
        .set_fault(Fault::Response(ResponseFault {
            route: Route::Presets,
            status: 200,
            body: b"[]".to_vec(),
        }))
        .await;

    // When: the cache reaches its cadence boundary and discovery returns a changed catalog.
    let changed = wait_for_worker(&fixture, registered.id, |record| {
        record.online && record.capabilities.workflows.is_empty()
    })
    .await?;

    // Then: the changed catalog replaces durable compatibility instead of stale cache evidence.
    assert!(changed.capabilities.refreshed_at.is_some());
    assert!(worker.counters().await.get(Route::Workflows) >= 2);
    assert!(worker.counters().await.get(Route::Presets) >= 2);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn offline_worker_does_not_block_healthy_peer_or_shutdown() -> TestResult {
    // Given: one worker returns failures while a second worker is healthy.
    let offline = MockVidenoa::start().await?;
    offline
        .set_fault(Fault::Response(ResponseFault {
            route: Route::Health,
            status: 503,
            body: Vec::new(),
        }))
        .await;
    let healthy = MockVidenoa::start().await?;
    let fixture = ControllerFixture::start().await?;
    let failed = fixture
        .register_worker_without_wait(&offline, "health-offline", true)
        .await?;
    let ready = fixture.register_worker(&healthy, "health-healthy").await?;

    // When: both independent probes execute and the runtime is stopped normally.
    wait_for_worker(&fixture, failed.id, |record| record.health_retry_count == 1).await?;
    wait_for_worker(&fixture, ready.id, |record| record.online).await?;
    fixture.stop().await?;

    // Then: graceful shutdown joined the health service without leaking its admitted stages.
    Ok(())
}

async fn wait_for_worker(
    fixture: &ControllerFixture,
    id: WorkerId,
    predicate: impl Fn(&videnoa_controller::persistence::WorkerRecord) -> bool,
) -> TestResult<videnoa_controller::persistence::WorkerRecord> {
    wait_for(|| async {
        fixture
            .store
            .worker(id)
            .await
            .ok()
            .flatten()
            .filter(&predicate)
    })
    .await
}

async fn wait_for_task(
    fixture: &ControllerFixture,
    task: &videnoa_controller::domain::Task,
    predicate: impl Fn(TaskStatus) -> bool,
) -> TestResult<videnoa_controller::domain::TaskDetailResponse> {
    wait_for(|| async {
        fixture
            .task(task)
            .await
            .ok()
            .filter(|detail| predicate(detail.task.status))
    })
    .await
}

async fn wait_for<T, Observe, Observed>(mut observe: Observe) -> TestResult<T>
where
    Observe: FnMut() -> Observed,
    Observed: Future<Output = Option<T>>,
{
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(value) = observe().await {
                return value;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .map_err(|_| std::io::Error::other("timed out waiting for durable runtime state").into())
}
