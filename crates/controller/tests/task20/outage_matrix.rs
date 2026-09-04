use std::sync::Arc;

use videnoa_controller::domain::TaskStatus;
use videnoa_controller::scheduler::{TransferCheckpointObserver, TransferCheckpointPoint};

use crate::mock_videnoa::faults::{DeleteOutcome, Fault, OfflineMode, ResponseFault};
use crate::mock_videnoa::journal::{HeaderValueSnapshot, Route};
use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{
    assert_completed_pipeline, assert_restarted_pipeline, complete_mock_job, CheckpointGate,
    ControllerFixture, TestResult,
};

#[path = "outage_matrix/waits.rs"]
mod waits;

use waits::{
    wait_for_remote_job, wait_for_run_journal_entries, wait_for_run_requests,
    wait_for_worker_offline,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn real_tcp_transfer_outages_retry_without_compute_replay() -> TestResult {
    run_fault(
        "upload-outage",
        Fault::DisconnectBeforeAccept,
        Route::Upload,
    )
    .await?;
    run_submission_fault("submit-outage").await?;
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
    Ok(())
}

async fn run_submission_fault(name: &str) -> TestResult {
    let worker = MockVidenoa::start_persistent().await?;
    worker.set_fault(Fault::AcceptThenDropRunResponse).await;
    let mut fixture = ControllerFixture::start().await?;
    fixture.register_worker(&worker, name).await?;
    let task = fixture.create_task(name, b"input-video").await?;
    wait_for_run_journal_entries(&worker, 1).await?;
    fixture.crash().await?;
    fixture.restart().await?;
    wait_for_run_requests(&worker, 2).await?;
    complete_mock_job(&worker, &task, b"enhanced-video").await?;
    assert_restarted_pipeline(&fixture, &worker, &task, b"enhanced-video").await?;
    let counters = worker.counters().await;
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
