#![expect(
    dead_code,
    reason = "focused scheduling tests reuse the complete Task 20 harness"
)]
#![expect(
    unused_imports,
    reason = "shared Task 20 support re-exports helpers for its full test matrix"
)]

#[path = "support/mock_videnoa/mod.rs"]
mod mock_videnoa;
#[path = "task20/support/mod.rs"]
mod support;

use std::time::Duration;

use mock_videnoa::checkpoints::Checkpoint;
use mock_videnoa::journal::Route;
use mock_videnoa::server::MockVidenoa;
use support::{complete_mock_job, ControllerFixture, TestResult};
use videnoa_controller::domain::{Task, TaskStatus};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn downloading_releases_compute_while_processing_and_prefetch_continue() -> TestResult {
    // Given: one compute slot with task A processing and task B prefetched on the same worker.
    let worker = MockVidenoa::start_persistent().await?;
    let fixture = ControllerFixture::start().await?;
    let registered = fixture.register_worker(&worker, "capacity-one").await?;
    let first_poll = worker.pause(Checkpoint::BeforePollResponse).await;
    let task_a = fixture.create_task("capacity-a", b"input-a").await?;
    worker.await_checkpoint(&first_poll).await?;
    wait_for_status(&fixture, &task_a, |status| status == TaskStatus::Processing).await?;
    let task_b = fixture.create_task("capacity-b", b"input-b").await?;
    wait_for_status(&fixture, &task_b, |status| {
        matches!(status, TaskStatus::Uploading | TaskStatus::Staged)
    })
    .await?;
    assert_eq!(worker.counters().await.get(Route::Run), 1);
    let busy_capacity = fixture
        .workers()
        .await?
        .into_iter()
        .find(|worker| worker.id == registered.id)
        .ok_or_else(|| std::io::Error::other("registered worker missing while busy"))?
        .capacity;
    assert_eq!(busy_capacity.used_slots, 1);
    assert!(busy_capacity.staged_tasks + u32::from(busy_capacity.active_uploads) <= 1);

    let download = worker.pause(Checkpoint::BeforeDownloadBody).await;
    let second_run = worker
        .pause(Checkpoint::AfterRunPersistedBeforeResponse)
        .await;

    // When: A completes remotely, moves to download, and the same prefetched B claims compute.
    complete_mock_job(&worker, &task_a, b"output-a").await?;
    worker.release(first_poll).await?;
    worker.await_checkpoint(&download).await?;
    worker.await_checkpoint(&second_run).await?;
    let second_poll = worker.pause(Checkpoint::BeforePollResponse).await;
    worker.release(second_run).await?;
    worker.await_checkpoint(&second_poll).await?;
    let task_c = fixture.create_task("capacity-c", b"input-c").await?;
    wait_for_status(&fixture, &task_c, |status| {
        matches!(status, TaskStatus::Uploading | TaskStatus::Staged)
    })
    .await?;

    // Then: C stages in behind B, every task keeps one worker, and remote AI never exceeds one.
    assert_eq!(
        fixture.task(&task_a).await?.task.status,
        TaskStatus::Downloading
    );
    assert_eq!(
        fixture.task(&task_b).await?.task.status,
        TaskStatus::Processing
    );
    assert!(matches!(
        fixture.task(&task_c).await?.task.status,
        TaskStatus::Uploading | TaskStatus::Staged
    ));
    for task in [&task_a, &task_b, &task_c] {
        assert_eq!(
            fixture.task(task).await?.task.worker_id,
            Some(registered.id)
        );
    }
    let workers = fixture.workers().await?;
    let capacity = &workers
        .iter()
        .find(|worker| worker.id == registered.id)
        .ok_or_else(|| std::io::Error::other("registered worker missing from API"))?
        .capacity;
    assert_eq!(capacity.used_slots, 1);
    assert_eq!(capacity.active_downloads, 1);
    assert_eq!(capacity.assigned_tasks, 3);
    assert!(capacity.staged_tasks + u32::from(capacity.active_uploads) <= 1);
    assert_eq!(worker.active_job_count().await, 1);
    assert_eq!(worker.peak_active_jobs().await, 1);
    assert_eq!(worker.counters().await.get(Route::Run), 2);

    worker.release(second_poll).await?;
    worker.release(download).await?;
    fixture.stop().await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_compute_slots_keep_one_prefetch_without_third_remote_job() -> TestResult {
    // Given: a two-slot worker with the configured single prefetch allowance.
    let worker = MockVidenoa::start_persistent().await?;
    let fixture = ControllerFixture::start().await?;
    let registered = fixture
        .register_worker_with_slots(&worker, "capacity-two", 2)
        .await?;
    let poll = worker.pause(Checkpoint::BeforePollResponse).await;

    // When: four tasks compete for two compute slots plus one stage-in reservation.
    let tasks = [
        fixture.create_task("slots-two-a", b"input-a").await?,
        fixture.create_task("slots-two-b", b"input-b").await?,
        fixture.create_task("slots-two-c", b"input-c").await?,
        fixture.create_task("slots-two-d", b"input-d").await?,
    ];
    worker.await_checkpoint(&poll).await?;
    wait_for_distribution(&fixture, &tasks).await?;

    // Then: two tasks process, one is staged in, one stays queued, and only two remote jobs exist.
    let mut processing = 0;
    let mut stage_in = 0;
    let mut queued = 0;
    for task in &tasks {
        match fixture.task(task).await?.task.status {
            TaskStatus::Processing => processing += 1,
            TaskStatus::Reserved | TaskStatus::Uploading | TaskStatus::Staged => stage_in += 1,
            TaskStatus::Queued => queued += 1,
            status => {
                return Err(std::io::Error::other(format!("unexpected status {status:?}")).into())
            }
        }
    }
    assert_eq!((processing, stage_in, queued), (2, 1, 1));
    assert_eq!(worker.active_job_count().await, 2);
    assert_eq!(worker.job_count().await, 2);
    let workers = fixture.workers().await?;
    let capacity = &workers
        .iter()
        .find(|worker| worker.id == registered.id)
        .ok_or_else(|| std::io::Error::other("registered worker missing from API"))?
        .capacity;
    assert_eq!(capacity.used_slots, 2);
    assert_eq!(
        capacity.staged_tasks + u32::from(capacity.active_uploads),
        1
    );

    worker.release(poll).await?;
    fixture.stop().await
}

async fn wait_for_status(
    fixture: &ControllerFixture,
    task: &Task,
    predicate: impl Fn(TaskStatus) -> bool,
) -> TestResult {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if predicate(fixture.task(task).await?.task.status) {
                return TestResult::Ok(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| std::io::Error::other("task did not reach expected scheduling state"))?
}

async fn wait_for_distribution(fixture: &ControllerFixture, tasks: &[Task; 4]) -> TestResult {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let mut processing = 0;
            let mut stage_in = 0;
            let mut queued = 0;
            for task in tasks {
                match fixture.task(task).await?.task.status {
                    TaskStatus::Processing => processing += 1,
                    TaskStatus::Reserved | TaskStatus::Uploading | TaskStatus::Staged => {
                        stage_in += 1;
                    }
                    TaskStatus::Queued => queued += 1,
                    TaskStatus::Submitting
                    | TaskStatus::RemoteCompleted
                    | TaskStatus::Downloading
                    | TaskStatus::Verifying
                    | TaskStatus::Publishing
                    | TaskStatus::RemoteCleanup
                    | TaskStatus::Completed
                    | TaskStatus::Failed
                    | TaskStatus::Cancelled => {}
                }
            }
            if (processing, stage_in, queued) == (2, 1, 1) {
                return TestResult::Ok(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| std::io::Error::other("tasks did not reach expected capacity distribution"))?
}
