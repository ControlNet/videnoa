use std::sync::Arc;
use std::time::Duration;

use videnoa_controller::domain::{Task, TaskStatus};
use videnoa_controller::scheduler::{TransferCheckpointObserver, TransferCheckpointPoint};

use crate::mock_videnoa::faults::{DeleteOutcome, Fault, OfflineMode, ResponseFault};
use crate::mock_videnoa::journal::{HeaderValueSnapshot, Route};
use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{
    assert_completed_pipeline, assert_restarted_pipeline, complete_mock_job, CheckpointGate,
    ControllerFixture, TestResult,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn real_tcp_transfer_outages_retry_without_compute_replay() -> TestResult {
    run_fault(
        "upload-outage",
        Fault::DisconnectBeforeAccept,
        Route::Upload,
    )
    .await?;
    run_fault(
        "submit-outage",
        Fault::AcceptThenDropRunResponse,
        Route::Run,
    )
    .await?;
    run_fault(
        "poll-outage",
        Fault::Response(ResponseFault {
            route: Route::JobPoll,
            status: 503,
            body: br#"{"error":"unavailable"}"#.to_vec(),
        }),
        Route::JobPoll,
    )
    .await?;
    run_download_outage("download-outage").await?;
    run_download_outage("retry-preserves-compute-identity").await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn cleanup_outages_converge_without_replaying_compute() -> TestResult {
    run_cleanup_outage("cleanup-404", vec![DeleteOutcome::NotFound], 1).await?;
    run_cleanup_outage(
        "cleanup-5xx",
        vec![DeleteOutcome::ServerError, DeleteOutcome::Success],
        2,
    )
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn health_outage_retains_assignment_until_worker_recovers() -> TestResult {
    let mut worker = MockVidenoa::start_persistent().await?;
    let fixture = ControllerFixture::start().await?;
    let registered = fixture.register_worker(&worker, "health-outage").await?;
    worker.set_offline(OfflineMode::ConnectionRefused).await?;
    let task = fixture.create_task("health-outage", b"input-video").await?;
    wait_for_worker_offline(&fixture, registered.id).await?;
    let detail = fixture.task(&task).await?;
    assert_eq!(detail.task.worker_id, Some(registered.id));
    assert_eq!(worker.job_count().await, 0);
    worker.resume().await?;
    wait_for_remote_job(&worker).await?;
    complete_mock_job(&worker, &task, b"enhanced-video").await?;
    assert_completed_pipeline(&fixture, &worker, &task, b"enhanced-video").await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn persisted_pause_blocks_admission_then_resumes_same_task() -> TestResult {
    let worker = MockVidenoa::start_persistent().await?;
    let fixture = ControllerFixture::start().await?;
    fixture.register_worker(&worker, "paused-worker").await?;
    fixture.pause_scheduler().await?;
    let task = fixture.create_task("paused-task", b"input-video").await?;
    assert_eq!(fixture.task(&task).await?.task.status, TaskStatus::Queued);
    assert_eq!(worker.job_count().await, 0);
    fixture.resume_scheduler().await?;
    wait_for_remote_job(&worker).await?;
    complete_mock_job(&worker, &task, b"enhanced-video").await?;
    assert_completed_pipeline(&fixture, &worker, &task, b"enhanced-video").await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn late_cancellation_cannot_regress_mandatory_cleanup() -> TestResult {
    let worker = MockVidenoa::start_persistent().await?;
    let gate = CheckpointGate::new(TransferCheckpointPoint::BeforeRemoteDelete);
    let observer: Arc<dyn TransferCheckpointObserver> = gate.clone();
    let fixture = ControllerFixture::start_with_checkpoint_observer(Some(observer)).await?;
    fixture.register_worker(&worker, "late-cancel").await?;
    let task = fixture.create_task("late-cancel", b"input-video").await?;
    wait_for_remote_job(&worker).await?;
    complete_mock_job(&worker, &task, b"enhanced-video").await?;
    gate.wait().await?;
    assert_eq!(
        fixture.cancel_status(&task).await?,
        reqwest::StatusCode::CONFLICT
    );
    gate.release();
    assert_completed_pipeline(&fixture, &worker, &task, b"enhanced-video").await
}

async fn run_fault(name: &str, fault: Fault, route: Route) -> TestResult {
    let worker = MockVidenoa::start_persistent().await?;
    worker.set_fault(fault).await;
    let fixture = ControllerFixture::start().await?;
    fixture.register_worker(&worker, name).await?;
    let task = fixture.create_task(name, b"input-video").await?;
    wait_for_remote_job(&worker).await?;
    complete_mock_job(&worker, &task, b"enhanced-video").await?;
    assert_restarted_pipeline(&fixture, &worker, &task, b"enhanced-video").await?;
    let counters = worker.counters().await;
    assert!(counters.get(route) >= 1);
    if route == Route::Run {
        assert_eq!(counters.get(Route::Run), 2);
        let keys = worker
            .journal()
            .await
            .into_iter()
            .filter(|entry| entry.route == Route::Run)
            .filter_map(|entry| {
                entry
                    .headers
                    .into_iter()
                    .find_map(|header| (header.name == "idempotency-key").then_some(header.value))
            })
            .collect::<Vec<_>>();
        assert_eq!(keys.len(), 2);
        assert!(matches!(
            keys.as_slice(),
            [HeaderValueSnapshot::Bytes(first), HeaderValueSnapshot::Bytes(second)] if first == second
        ));
        assert_eq!(worker.job_count().await, 1);
    }
    Ok(())
}

async fn run_download_outage(name: &str) -> TestResult {
    let worker = MockVidenoa::start_persistent().await?;
    worker
        .set_fault(Fault::TruncateDownload { delivered_bytes: 3 })
        .await;
    let fixture = ControllerFixture::start().await?;
    fixture.register_worker(&worker, name).await?;
    let task = fixture.create_task(name, b"input-video").await?;
    wait_for_remote_job(&worker).await?;
    complete_mock_job(&worker, &task, b"enhanced-video").await?;
    assert_completed_pipeline(&fixture, &worker, &task, b"enhanced-video").await?;
    assert!(worker.counters().await.get(Route::Download) >= 2);
    Ok(())
}

async fn run_cleanup_outage(
    name: &str,
    outcomes: Vec<DeleteOutcome>,
    expected_deletes: u64,
) -> TestResult {
    let worker = MockVidenoa::start_persistent().await?;
    worker.set_fault(Fault::DeleteScript(outcomes)).await;
    let gate = CheckpointGate::new(TransferCheckpointPoint::BeforeRemoteDelete);
    let observer: Arc<dyn TransferCheckpointObserver> = gate.clone();
    let fixture = ControllerFixture::start_with_checkpoint_observer(Some(observer)).await?;
    fixture.register_worker(&worker, name).await?;
    let task = fixture.create_task(name, b"input-video").await?;
    wait_for_remote_job(&worker).await?;
    complete_mock_job(&worker, &task, b"enhanced-video").await?;
    gate.wait().await?;
    let before_cleanup = worker.counters().await;
    assert_eq!(before_cleanup.get(Route::Upload), 1);
    assert_eq!(before_cleanup.get(Route::Run), 1);
    assert_eq!(before_cleanup.get(Route::Download), 1);
    gate.release();
    assert_completed_pipeline(&fixture, &worker, &task, b"enhanced-video").await?;
    let after_cleanup = worker.counters().await;
    assert_eq!(
        after_cleanup.get(Route::Upload),
        before_cleanup.get(Route::Upload)
    );
    assert_eq!(
        after_cleanup.get(Route::Run),
        before_cleanup.get(Route::Run)
    );
    assert_eq!(
        after_cleanup.get(Route::Download),
        before_cleanup.get(Route::Download)
    );
    assert_eq!(after_cleanup.get(Route::DeleteFile), expected_deletes);
    Ok(())
}

async fn wait_for_remote_job(worker: &MockVidenoa) -> TestResult {
    tokio::time::timeout(Duration::from_secs(10), async {
        while worker.job_count().await == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| std::io::Error::other("remote job was not created"))?;
    Ok(())
}

async fn wait_for_status(
    fixture: &ControllerFixture,
    task: &Task,
    status: TaskStatus,
) -> TestResult<videnoa_controller::domain::TaskDetailResponse> {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let detail = fixture.task(task).await?;
            if detail.task.status == status {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(detail);
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| std::io::Error::other("task did not reach expected status"))?
}

async fn wait_for_worker_offline(
    fixture: &ControllerFixture,
    worker_id: videnoa_controller::domain::WorkerId,
) -> TestResult {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if fixture
                .store
                .worker(worker_id)
                .await?
                .is_some_and(|worker| !worker.online)
            {
                return Ok::<_, videnoa_controller::persistence::PersistenceError>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| std::io::Error::other("worker was not marked offline"))??;
    Ok(())
}
