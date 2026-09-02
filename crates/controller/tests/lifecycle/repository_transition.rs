use videnoa_controller::domain::{RemoteJobId, RemotePath, TaskStatus};
use videnoa_controller::lifecycle::{
    AdvanceCommand, CancelAction, DurableAction, LifecycleErrorCode, SubmissionEvidence,
};

use super::support::{fixture, reserve, TestResult};

#[tokio::test]
async fn durable_transition_precedes_each_exposed_side_effect() -> TestResult {
    // Given: a reserved task and its durable first attempt.
    let fixture = fixture().await?;
    let attempt_id = reserve(&fixture).await?;
    let task = fixture
        .store
        .task(fixture.task_id)
        .await?
        .ok_or_else(|| std::io::Error::other("task missing"))?;
    let attempt = fixture
        .store
        .attempt(attempt_id)
        .await?
        .ok_or_else(|| std::io::Error::other("attempt missing"))?;

    // When: upload is authorized through the lifecycle service.
    let committed = fixture
        .service
        .advance(&task, &attempt, AdvanceCommand::StartUpload, fixture.now)
        .await?;

    // Then: the action is returned only after both durable rows are uploading.
    assert_eq!(committed.action(), DurableAction::Upload);
    assert_eq!(
        fixture
            .store
            .task(fixture.task_id)
            .await?
            .ok_or_else(|| std::io::Error::other("task missing"))?
            .status,
        TaskStatus::Uploading,
    );
    assert_eq!(
        fixture
            .store
            .attempt(attempt_id)
            .await?
            .ok_or_else(|| std::io::Error::other("attempt missing"))?
            .attempt
            .status,
        TaskStatus::Uploading,
    );
    Ok(())
}

#[tokio::test]
async fn illegal_and_stale_commands_cannot_commit() -> TestResult {
    // Given: one reserved task snapshot and attempt snapshot.
    let fixture = fixture().await?;
    let attempt_id = reserve(&fixture).await?;
    let task = fixture
        .store
        .task(fixture.task_id)
        .await?
        .ok_or_else(|| std::io::Error::other("task missing"))?;
    let attempt = fixture
        .store
        .attempt(attempt_id)
        .await?
        .ok_or_else(|| std::io::Error::other("attempt missing"))?;

    // When: an illegal stage jump and then a replayed legal CAS are attempted.
    let illegal = fixture
        .service
        .advance(&task, &attempt, AdvanceCommand::StartDownload, fixture.now)
        .await;
    fixture
        .service
        .advance(&task, &attempt, AdvanceCommand::StartUpload, fixture.now)
        .await?;
    let stale = fixture
        .service
        .advance(&task, &attempt, AdvanceCommand::StartUpload, fixture.now)
        .await;

    // Then: policy rejects the jump and repository CAS rejects the stale snapshot.
    assert_eq!(
        illegal.expect_err("illegal command must fail").code(),
        LifecycleErrorCode::IllegalCommand
    );
    assert_eq!(
        stale.expect_err("stale command must fail").code(),
        LifecycleErrorCode::Conflict
    );
    Ok(())
}

#[tokio::test]
async fn submission_evidence_is_bound_before_processing_is_exposed() -> TestResult {
    // Given: a staged attempt advanced into the durable submitting state.
    let fixture = fixture().await?;
    let attempt_id = reserve(&fixture).await?;
    let commands = [
        AdvanceCommand::StartUpload,
        AdvanceCommand::FinishUpload,
        AdvanceCommand::StartSubmission,
    ];
    for command in commands {
        let task = fixture
            .store
            .task(fixture.task_id)
            .await?
            .ok_or_else(|| std::io::Error::other("task missing"))?;
        let attempt = fixture
            .store
            .attempt(attempt_id)
            .await?
            .ok_or_else(|| std::io::Error::other("attempt missing"))?;
        fixture
            .service
            .advance(&task, &attempt, command, fixture.now)
            .await?;
    }
    let task = fixture
        .store
        .task(fixture.task_id)
        .await?
        .ok_or_else(|| std::io::Error::other("task missing"))?;
    let attempt = fixture
        .store
        .attempt(attempt_id)
        .await?
        .ok_or_else(|| std::io::Error::other("attempt missing"))?;
    let remote_job_id = RemoteJobId::random();

    // When: accepted submission evidence is committed with the processing transition.
    let committed = fixture
        .service
        .advance(
            &task,
            &attempt,
            AdvanceCommand::PersistSubmission(SubmissionEvidence {
                remote_job_id,
                remote_input_path: RemotePath::new("task/input/../opaque.mkv"),
                remote_output_path: RemotePath::new("task/output/../opaque.mp4"),
            }),
            fixture.now,
        )
        .await?;

    // Then: polling is exposed only with the exact remote evidence durable.
    let stored = fixture
        .store
        .attempt(attempt_id)
        .await?
        .ok_or_else(|| std::io::Error::other("attempt missing"))?;
    assert_eq!(committed.action(), DurableAction::Poll);
    assert_eq!(stored.attempt.status, TaskStatus::Processing);
    assert_eq!(stored.attempt.remote_job_id, Some(remote_job_id));
    assert_eq!(
        stored
            .attempt
            .remote_input_path
            .as_ref()
            .map(RemotePath::as_str),
        Some("task/input/../opaque.mkv")
    );
    Ok(())
}

#[tokio::test]
async fn active_cancellation_persists_intent_before_cleanup_and_then_closes_attempt() -> TestResult
{
    // Given: an uploading task with no prior cancellation intent.
    let fixture = fixture().await?;
    let attempt_id = reserve(&fixture).await?;
    let task = fixture
        .store
        .task(fixture.task_id)
        .await?
        .ok_or_else(|| std::io::Error::other("task missing"))?;
    let attempt = fixture
        .store
        .attempt(attempt_id)
        .await?
        .ok_or_else(|| std::io::Error::other("attempt missing"))?;
    fixture
        .service
        .advance(&task, &attempt, AdvanceCommand::StartUpload, fixture.now)
        .await?;
    let task = fixture
        .store
        .task(fixture.task_id)
        .await?
        .ok_or_else(|| std::io::Error::other("task missing"))?;
    let attempt = fixture
        .store
        .attempt(attempt_id)
        .await?
        .ok_or_else(|| std::io::Error::other("attempt missing"))?;

    // When: cancellation is requested and cleanup later reports completion.
    let requested = fixture
        .service
        .request_cancellation(&task, Some(&attempt), fixture.now)
        .await?;
    let marked_task = fixture
        .store
        .task(fixture.task_id)
        .await?
        .ok_or_else(|| std::io::Error::other("task missing"))?;
    let marked_attempt = fixture
        .store
        .attempt(attempt_id)
        .await?
        .ok_or_else(|| std::io::Error::other("attempt missing"))?;
    fixture
        .service
        .finish_cancellation(&marked_task, &marked_attempt, fixture.now)
        .await?;

    // Then: cleanup authorization follows durable intent and both rows end cancelled.
    assert_eq!(
        requested.action(),
        DurableAction::Cancel(CancelAction::AbortUploadAndClean)
    );
    assert_eq!(marked_task.status, TaskStatus::Uploading);
    assert_eq!(marked_task.cancel_requested_at, Some(fixture.now));
    assert_eq!(
        fixture
            .store
            .task(fixture.task_id)
            .await?
            .ok_or_else(|| std::io::Error::other("task missing"))?
            .status,
        TaskStatus::Cancelled
    );
    assert_eq!(
        fixture
            .store
            .attempt(attempt_id)
            .await?
            .ok_or_else(|| std::io::Error::other("attempt missing"))?
            .attempt
            .status,
        TaskStatus::Cancelled
    );
    Ok(())
}
