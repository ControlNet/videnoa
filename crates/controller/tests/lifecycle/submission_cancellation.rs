use videnoa_controller::domain::{RemoteJobId, RemotePath, TaskStatus};
use videnoa_controller::lifecycle::{
    AdvanceCommand, CancelAction, DurableAction, LifecycleErrorCode,
    SubmissionCancellationReconciliation, SubmissionEvidence,
};

use super::support::{
    fixture, load_attempt, load_task, request_submitting_cancellation, submitting_attempt,
    TestResult,
};

#[tokio::test]
async fn accepted_submission_after_cancellation_requires_reconciliation_path() -> TestResult {
    // Given: a submitting attempt with durable cancellation intent.
    let fixture = fixture().await?;
    let attempt_id = submitting_attempt(&fixture).await?;
    request_submitting_cancellation(&fixture, attempt_id).await?;
    let task = load_task(&fixture).await?;
    let attempt = load_attempt(&fixture, attempt_id).await?;
    let remote_job_id = RemoteJobId::random();

    // When: accepted keyed-submission evidence is reconciled after cancellation intent.
    let committed = fixture
        .service
        .reconcile_submission_cancellation(
            &task,
            &attempt,
            SubmissionCancellationReconciliation::Accepted(SubmissionEvidence {
                remote_job_id,
                remote_input_path: RemotePath::new("task/input/../cancel-opaque.mkv"),
                remote_output_path: RemotePath::new("task/output/../cancel-opaque.mp4"),
            }),
            fixture.now,
        )
        .await?;

    // Then: evidence is durable before remote cancellation and polling is never exposed.
    let reconciled_task = load_task(&fixture).await?;
    let reconciled_attempt = load_attempt(&fixture, attempt_id).await?;
    assert_eq!(
        committed.action(),
        DurableAction::Cancel(CancelAction::CancelRemoteAndClean)
    );
    assert_eq!(reconciled_task.status, TaskStatus::Processing);
    assert_eq!(reconciled_task.cancel_requested_at, Some(fixture.now));
    assert_eq!(
        reconciled_attempt.attempt.remote_job_id,
        Some(remote_job_id)
    );
    assert_eq!(
        reconciled_attempt
            .attempt
            .remote_input_path
            .as_ref()
            .map(RemotePath::as_str),
        Some("task/input/../cancel-opaque.mkv")
    );
    assert_eq!(
        reconciled_attempt
            .attempt
            .remote_output_path
            .as_ref()
            .map(RemotePath::as_str),
        Some("task/output/../cancel-opaque.mp4")
    );
    let continuation = fixture
        .service
        .advance(
            &reconciled_task,
            &reconciled_attempt,
            AdvanceCommand::FinishProcessing,
            fixture.now,
        )
        .await;
    assert_eq!(
        continuation
            .expect_err("processing continuation must remain blocked")
            .code(),
        LifecycleErrorCode::Conflict
    );
    fixture
        .service
        .finish_cancellation(&reconciled_task, &reconciled_attempt, fixture.now)
        .await?;
    assert_eq!(load_task(&fixture).await?.status, TaskStatus::Cancelled);
    Ok(())
}

#[tokio::test]
async fn not_accepted_submission_reconciliation_requires_staged_cleanup() -> TestResult {
    // Given: a submitting attempt with durable cancellation intent.
    let fixture = fixture().await?;
    let attempt_id = submitting_attempt(&fixture).await?;
    request_submitting_cancellation(&fixture, attempt_id).await?;
    let task = load_task(&fixture).await?;
    let attempt = load_attempt(&fixture, attempt_id).await?;

    // When: keyed reconciliation proves that no submission was accepted.
    let committed = fixture
        .service
        .reconcile_submission_cancellation(
            &task,
            &attempt,
            SubmissionCancellationReconciliation::NotAccepted,
            fixture.now,
        )
        .await?;

    // Then: staged cleanup is authorized before terminal cancellation can complete.
    let reconciled_task = load_task(&fixture).await?;
    let reconciled_attempt = load_attempt(&fixture, attempt_id).await?;
    assert_eq!(
        committed.action(),
        DurableAction::Cancel(CancelAction::CleanStaged)
    );
    assert_eq!(reconciled_task.status, TaskStatus::Staged);
    assert_eq!(reconciled_task.cancel_requested_at, Some(fixture.now));
    fixture
        .service
        .finish_cancellation(&reconciled_task, &reconciled_attempt, fixture.now)
        .await?;
    assert_eq!(load_task(&fixture).await?.status, TaskStatus::Cancelled);
    Ok(())
}

#[tokio::test]
async fn submitting_cancellation_cannot_finish_before_submission_reconciliation() -> TestResult {
    // Given: a submitting attempt with cancellation intent but no reconciliation result.
    let fixture = fixture().await?;
    let attempt_id = submitting_attempt(&fixture).await?;
    request_submitting_cancellation(&fixture, attempt_id).await?;
    let task = load_task(&fixture).await?;
    let attempt = load_attempt(&fixture, attempt_id).await?;

    // When: cancellation completion is requested directly.
    let result = fixture
        .service
        .finish_cancellation(&task, &attempt, fixture.now)
        .await;

    // Then: unproven reconciliation cannot close either durable row.
    assert_eq!(
        result
            .expect_err("submitting cancellation requires reconciliation")
            .code(),
        LifecycleErrorCode::IllegalCommand
    );
    assert_eq!(load_task(&fixture).await?.status, TaskStatus::Submitting);
    assert_eq!(
        load_attempt(&fixture, attempt_id).await?.attempt.status,
        TaskStatus::Submitting
    );
    Ok(())
}

#[tokio::test]
async fn ordinary_submission_continuation_remains_blocked_after_cancellation_intent() -> TestResult
{
    // Given: a submitting attempt with durable cancellation intent.
    let fixture = fixture().await?;
    let attempt_id = submitting_attempt(&fixture).await?;
    request_submitting_cancellation(&fixture, attempt_id).await?;
    let task = load_task(&fixture).await?;
    let attempt = load_attempt(&fixture, attempt_id).await?;

    // When: the ordinary processing continuation tries to expose polling.
    let result = fixture
        .service
        .advance(
            &task,
            &attempt,
            AdvanceCommand::PersistSubmission(SubmissionEvidence {
                remote_job_id: RemoteJobId::random(),
                remote_input_path: RemotePath::new("task/input.mkv"),
                remote_output_path: RemotePath::new("task/output.mp4"),
            }),
            fixture.now,
        )
        .await;

    // Then: the blanket cancellation safety still blocks the ordinary command path.
    assert_eq!(
        result.expect_err("ordinary continuation must fail").code(),
        LifecycleErrorCode::Conflict
    );
    assert_eq!(load_task(&fixture).await?.status, TaskStatus::Submitting);
    Ok(())
}

#[tokio::test]
async fn stale_submission_reconciliation_snapshot_cannot_commit_twice() -> TestResult {
    // Given: one submitting cancellation snapshot and a proven not-accepted result.
    let fixture = fixture().await?;
    let attempt_id = submitting_attempt(&fixture).await?;
    request_submitting_cancellation(&fixture, attempt_id).await?;
    let task = load_task(&fixture).await?;
    let attempt = load_attempt(&fixture, attempt_id).await?;

    // When: the same reconciliation CAS is attempted twice.
    fixture
        .service
        .reconcile_submission_cancellation(
            &task,
            &attempt,
            SubmissionCancellationReconciliation::NotAccepted,
            fixture.now,
        )
        .await?;
    let stale = fixture
        .service
        .reconcile_submission_cancellation(
            &task,
            &attempt,
            SubmissionCancellationReconciliation::NotAccepted,
            fixture.now,
        )
        .await;

    // Then: only the first reconciliation changes durable state.
    assert_eq!(
        stale.expect_err("stale reconciliation must fail").code(),
        LifecycleErrorCode::Conflict
    );
    assert_eq!(load_task(&fixture).await?.status, TaskStatus::Staged);
    Ok(())
}
