use crate::mock_videnoa::checkpoints::Checkpoint;
use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{assert_completed_pipeline, complete_mock_job, ControllerFixture, TestResult};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_worker_real_http_pipeline_converges_and_retains_proof() -> TestResult {
    // Given: an authenticated real-TCP Controller and one eligible persistent worker.
    let worker = MockVidenoa::start_persistent().await?;
    let fixture = ControllerFixture::start().await?;
    let registered = fixture.register_worker(&worker, "worker-one").await?;
    let run = worker
        .pause(Checkpoint::AfterRunPersistedBeforeResponse)
        .await;
    let deleted = worker.pause(Checkpoint::AfterDelete).await;

    // When: intake is driven through POST /api/tasks.
    let task = fixture.create_task("one-worker", b"input-video").await?;
    let before_dispatch = fixture.task(&task).await?;
    let counters = worker.counters().await;
    eprintln!(
        "task20_pre_dispatch status={:?} attempts={} run_requests={} remote_jobs={}",
        before_dispatch.task.status,
        before_dispatch.attempts.len(),
        counters.get(crate::mock_videnoa::journal::Route::Run),
        worker.job_count().await
    );

    // Then: the production runtime must dispatch keyed compute and converge cleanup.
    worker.await_checkpoint(&run).await.map_err(|error| {
        std::io::Error::other(format!(
            "Controller never dispatched the queued HTTP task to /api/run: {error}"
        ))
    })?;
    assert_eq!(fixture.store.worker_used_slots(registered.id).await?, 1);
    complete_mock_job(&worker, &task, b"enhanced-video").await?;
    worker.release(run).await?;
    if let Err(error) = worker.await_checkpoint(&deleted).await {
        let detail = fixture.task(&task).await?;
        return Err(std::io::Error::other(format!(
            "Controller did not reach remote cleanup after run reconciliation: {error}; status={:?} failure={:?}",
            detail.task.status, detail.task.failure
        ))
        .into());
    }
    worker.release(deleted).await?;
    assert_completed_pipeline(&fixture, &worker, &task, b"enhanced-video").await?;
    worker.write_happy_evidence_if_requested().await?;
    assert_eq!(fixture.store.worker_used_slots(registered.id).await?, 0);
    Ok(())
}
