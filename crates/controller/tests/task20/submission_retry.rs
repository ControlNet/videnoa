use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use videnoa_controller::domain::{Task, TaskStatus};
use videnoa_controller::lifecycle::LifecycleService;
use videnoa_controller::scheduler::{TransferCheckpointObserver, TransferCheckpointPoint};

use crate::mock_videnoa::checkpoints::Checkpoint;
use crate::mock_videnoa::faults::{Fault, ResponseFault};
use crate::mock_videnoa::journal::Route;
use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{CheckpointGate, ControllerFixture, TestResult};

async fn staged_fixture(name: &str) -> TestResult<(ControllerFixture, MockVidenoa, Task)> {
    let gate = CheckpointGate::new(TransferCheckpointPoint::BeforeRemoteSubmit);
    let observer: Arc<dyn TransferCheckpointObserver> = gate.clone();
    let worker = MockVidenoa::start_persistent().await?;
    let mut fixture = ControllerFixture::start_with_checkpoint_observer(Some(observer)).await?;
    fixture.register_worker(&worker, name).await?;
    let task = fixture.create_task(name, b"input-video").await?;
    gate.wait().await?;
    fixture.crash().await?;
    gate.release();
    Ok((fixture, worker, task))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn transient_submission_failures_back_off_and_preserve_identity() -> TestResult {
    let (fixture, worker, task) = staged_fixture("retry-backoff").await?;
    let mut config = fixture.store.config_manager().config();
    config.retry.initial = Duration::from_secs(1);
    config.retry.maximum = Duration::from_secs(2);
    config.retry.max_attempts = NonZeroU32::new(1).expect("nonzero");
    fixture.store.config_manager().initialize(config, None);
    let reconciler = fixture.reconciler()?;
    let original = fixture
        .store
        .current_attempt(task.id)
        .await?
        .expect("attempt");
    let mut now = Utc::now();

    for (index, status) in [503, 429, 502, 503].into_iter().enumerate() {
        worker
            .set_fault(Fault::Response(ResponseFault {
                route: Route::Run,
                status,
                body: b"{}".to_vec(),
            }))
            .await;
        let result = reconciler.reconcile_task_id(task.id, now).await;
        if status == 429 {
            result?;
        } else {
            assert!(result.is_err());
        }
        let attempt = fixture
            .store
            .current_attempt(task.id)
            .await?
            .expect("attempt");
        let durable_task = fixture.store.task(task.id).await?.expect("task");
        assert_eq!(attempt.attempt.id, original.attempt.id);
        assert_eq!(
            attempt.attempt.submission_key,
            original.attempt.submission_key
        );
        assert_eq!(attempt.attempt.retry.retry_count as usize, index + 1);
        assert_eq!(durable_task.retry, attempt.attempt.retry);
        assert_eq!(durable_task.status, TaskStatus::Submitting);
        let retry_at = attempt.attempt.retry.next_retry_at.expect("retry deadline");
        let expected_delay = if index == 0 { 1 } else { 2 };
        assert_eq!(
            (retry_at - attempt.updated_at).num_seconds(),
            expected_delay
        );

        // Neither the current generation nor a restart can bypass backoff.
        reconciler
            .reconcile_task_id(task.id, retry_at - chrono::Duration::milliseconds(1))
            .await?;
        fixture
            .reconciler()?
            .reconcile_task_id(task.id, retry_at - chrono::Duration::milliseconds(1))
            .await?;
        assert_eq!(
            worker.counters().await.get(Route::Run),
            u64::try_from(index + 1)?
        );
        assert_eq!(worker.job_count().await, 0);
        if index == 0 {
            let mut config = fixture.store.config_manager().config();
            config.scheduler.paused = true;
            fixture.store.config_manager().initialize(config, None);
        }
        now = retry_at;
    }
    reconciler.reconcile_task_id(task.id, now).await?;
    let recovered = fixture
        .store
        .current_attempt(task.id)
        .await?
        .expect("attempt");
    assert_eq!(recovered.attempt.status, TaskStatus::Processing);
    assert_eq!(recovered.attempt.retry.retry_count, 0);
    assert!(recovered.attempt.retry.next_retry_at.is_none());
    assert_eq!(worker.job_count().await, 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn active_submission_is_exclusive_and_receipt_conflict_can_recover() -> TestResult {
    let (fixture, worker, task) = staged_fixture("retry-receipt-conflict").await?;
    let held = worker
        .pause(Checkpoint::AfterRunPersistedBeforeResponse)
        .await;
    let reconciler = fixture.reconciler()?;
    let concurrent = reconciler.clone();
    let task_id = task.id;
    let submitting =
        tokio::spawn(async move { concurrent.reconcile_task_id(task_id, Utc::now()).await });
    worker.await_checkpoint(&held).await?;
    let report = reconciler.reconcile_task_id(task.id, Utc::now()).await?;
    assert!(report
        .deferred()
        .iter()
        .any(|entry| entry.task_id == task.id));
    assert_eq!(worker.counters().await.get(Route::Run), 1);

    // A cancellation arriving before the response changes the task version.
    let durable_task = fixture.store.task(task.id).await?.expect("task");
    let attempt = fixture
        .store
        .current_attempt(task.id)
        .await?
        .expect("attempt");
    LifecycleService::new(fixture.store.clone())
        .request_cancellation(&durable_task, Some(&attempt), Utc::now())
        .await?;
    worker.release(held).await?;
    submitting.await??;
    let attempt = fixture
        .store
        .current_attempt(task.id)
        .await?
        .expect("attempt");
    assert_eq!(attempt.attempt.retry.retry_count, 1);
    let retry_at = attempt.attempt.retry.next_retry_at.expect("retry deadline");
    reconciler.reconcile_task_id(task.id, retry_at).await?;
    assert_eq!(
        fixture.store.task(task.id).await?.expect("task").status,
        TaskStatus::Cancelled
    );
    assert_eq!(worker.counters().await.get(Route::Run), 2);
    assert_eq!(worker.counters().await.get(Route::JobCancel), 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rejected_submission_does_not_schedule_automatic_retry() -> TestResult {
    let (fixture, worker, task) = staged_fixture("retry-rejected").await?;
    worker
        .set_fault(Fault::Response(ResponseFault {
            route: Route::Run,
            status: 400,
            body: b"{}".to_vec(),
        }))
        .await;
    let reconciler = fixture.reconciler()?;
    reconciler.reconcile_task_id(task.id, Utc::now()).await?;
    let attempt = fixture
        .store
        .current_attempt(task.id)
        .await?
        .expect("attempt");
    assert_eq!(attempt.attempt.status, TaskStatus::Failed);
    assert!(attempt.attempt.retry.next_retry_at.is_none());
    assert_eq!(worker.counters().await.get(Route::Run), 1);
    assert_eq!(worker.job_count().await, 0);
    Ok(())
}
