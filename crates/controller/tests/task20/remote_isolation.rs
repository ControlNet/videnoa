use std::time::Duration;

use videnoa_controller::domain::{FailureCode, FailureStage, Task, TaskStatus};

use crate::mock_videnoa::checkpoints::Checkpoint;
use crate::mock_videnoa::faults::{Fault, ResponseFault};
use crate::mock_videnoa::journal::{HeaderValueSnapshot, Route};
use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{
    assert_completed_pipeline, assert_restarted_pipeline, complete_mock_job, ControllerFixture,
    TestResult,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn rejected_submission_fails_only_its_task_while_other_work_completes() -> TestResult {
    // Given: one occupied worker will reject submission while another can run normally.
    let bad_worker = MockVidenoa::start_persistent().await?;
    let good_worker = MockVidenoa::start_persistent().await?;
    let fixture = ControllerFixture::start().await?;
    fixture
        .register_worker(&bad_worker, "submission-reject")
        .await?;
    let bad_run = bad_worker.pause(Checkpoint::BeforeRunPersistence).await;
    let bad_task = fixture
        .create_task("submission-reject", b"bad-input")
        .await?;
    bad_worker.await_checkpoint(&bad_run).await?;

    fixture
        .register_worker(&good_worker, "submission-good")
        .await?;
    let good_run = good_worker
        .pause(Checkpoint::AfterRunPersistedBeforeResponse)
        .await;
    let good_task = fixture
        .create_task("submission-good", b"good-input")
        .await?;
    good_worker.await_checkpoint(&good_run).await?;
    complete_mock_job(&good_worker, &good_task, b"good-output").await?;
    bad_worker
        .set_fault(Fault::Response(ResponseFault {
            route: Route::Run,
            status: 400,
            body: br#"{"error":"workflow validation failed: node 'output' missing required input port 'output_path'"}"#.to_vec(),
        }))
        .await;

    // When: both workers release their pending submissions.
    bad_worker.release(bad_run).await?;
    good_worker.release(good_run).await?;

    // Then: rejection is durable and the unrelated task completes through the live Orchestrator.
    let failed = wait_for_status(&fixture, &bad_task, TaskStatus::Failed).await?;
    let failure = failed
        .task
        .failure
        .ok_or_else(|| std::io::Error::other("failed task has no durable failure"))?;
    assert_eq!(failure.failure_stage, FailureStage::Submission);
    assert_eq!(failure.failure_code, FailureCode::RemoteSubmissionFailed);
    assert!(!failure.retryable);
    assert!(
        failure.message.ends_with(
            ": workflow validation failed: node 'output' missing required input port 'output_path'"
        ),
        "the Worker's rejection reason should reach the task: {}",
        failure.message
    );
    assert_completed_pipeline(&fixture, &good_worker, &good_task, b"good-output").await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn malformed_poll_fails_only_its_task_while_other_work_completes() -> TestResult {
    // Given: one processing task is paused before a malformed poll response.
    let bad_worker = MockVidenoa::start_persistent().await?;
    let good_worker = MockVidenoa::start_persistent().await?;
    let fixture = ControllerFixture::start().await?;
    fixture
        .register_worker(&bad_worker, "poll-malformed")
        .await?;
    let bad_run = bad_worker
        .pause(Checkpoint::AfterRunPersistedBeforeResponse)
        .await;
    let bad_poll = bad_worker.pause(Checkpoint::BeforePollResponse).await;
    let bad_task = fixture.create_task("poll-malformed", b"bad-input").await?;
    bad_worker.await_checkpoint(&bad_run).await?;
    bad_worker.release(bad_run).await?;
    bad_worker.await_checkpoint(&bad_poll).await?;

    fixture.register_worker(&good_worker, "poll-good").await?;
    let good_run = good_worker
        .pause(Checkpoint::AfterRunPersistedBeforeResponse)
        .await;
    let good_task = fixture.create_task("poll-good", b"good-input").await?;
    good_worker.await_checkpoint(&good_run).await?;
    complete_mock_job(&good_worker, &good_task, b"good-output").await?;
    bad_worker
        .set_fault(Fault::Response(ResponseFault {
            route: Route::JobPoll,
            status: 200,
            body: br#"{"status":"running"}"#.to_vec(),
        }))
        .await;

    // When: the malformed poll and successful submission responses are released.
    bad_worker.release(bad_poll).await?;
    good_worker.release(good_run).await?;

    // Then: remote ambiguity is durable and the unrelated task completes normally.
    let failed = wait_for_status(&fixture, &bad_task, TaskStatus::Failed).await?;
    let failure = failed
        .task
        .failure
        .ok_or_else(|| std::io::Error::other("failed task has no durable failure"))?;
    assert_eq!(failure.failure_stage, FailureStage::Processing);
    assert_eq!(failure.failure_code, FailureCode::RemoteStateAmbiguous);
    assert!(!failure.retryable);
    assert_completed_pipeline(&fixture, &good_worker, &good_task, b"good-output").await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn rejected_submission_resubmits_on_manual_retry_without_reupload() -> TestResult {
    // Given: a worker that rejects the task's first submission.
    let worker = MockVidenoa::start_persistent().await?;
    let fixture = ControllerFixture::start().await?;
    worker
        .set_fault(Fault::Response(ResponseFault {
            route: Route::Run,
            status: 400,
            body: br#"{"error":"workflow validation failed"}"#.to_vec(),
        }))
        .await;
    fixture.register_worker(&worker, "submission-fixed").await?;
    let task = fixture
        .create_task("submission-fixed", b"fixed-input")
        .await?;
    let failed = wait_for_status(&fixture, &task, TaskStatus::Failed).await?;
    assert_eq!(
        failed.task.failure.map(|failure| failure.failure_code),
        Some(FailureCode::RemoteSubmissionFailed)
    );
    let original = failed
        .attempts
        .first()
        .ok_or_else(|| std::io::Error::other("failed attempt missing"))?
        .clone();

    // When: the cause is fixed on the worker and the user retries.
    let retried = fixture.retry_task(&task).await?;
    complete_mock_job(&worker, &task, b"fixed-output").await?;

    // Then: the same attempt resubmits with its key, without a second upload.
    assert_eq!(retried.attempt_id, Some(original.id));
    assert_restarted_pipeline(&fixture, &worker, &task, b"fixed-output").await?;
    let counters = worker.counters().await;
    assert_eq!(counters.get(Route::Run), 2);
    assert_eq!(counters.get(Route::Upload), 1);
    assert_eq!(worker.job_count().await, 1);
    let keys: Vec<String> = worker
        .journal()
        .await
        .into_iter()
        .filter(|entry| entry.route == Route::Run)
        .filter_map(|entry| {
            entry
                .headers
                .into_iter()
                .find(|header| header.name == "idempotency-key")
                .and_then(|header| match header.value {
                    HeaderValueSnapshot::Bytes(bytes) => String::from_utf8(bytes).ok(),
                    HeaderValueSnapshot::Redacted => None,
                })
        })
        .collect();
    assert_eq!(keys, vec![original.submission_key.to_string(); 2]);
    Ok(())
}

async fn wait_for_status(
    fixture: &ControllerFixture,
    task: &Task,
    expected: TaskStatus,
) -> TestResult<videnoa_controller::domain::TaskDetailResponse> {
    tokio::time::timeout(
        crate::mock_videnoa::deadline::eventually(Duration::from_secs(15)),
        async {
            loop {
                let detail = fixture.task(task).await?;
                if detail.task.status == expected {
                    return Ok(detail);
                }
                tokio::task::yield_now().await;
            }
        },
    )
    .await
    .map_err(|_| std::io::Error::other(format!("task did not reach {expected:?}")))?
}
