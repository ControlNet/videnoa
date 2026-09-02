use videnoa_controller::domain::{AttemptId, RemoteJobId, SubmissionKey, TaskStatus};
use videnoa_controller::lifecycle::{
    AdvanceCommand, DownstreamFailure, DurableAction, LifecycleFailure, ProcessingRetryCommand,
    RemoteTerminalStatus, TerminalRemoteEvidence, WorkspaceCleaned,
};

use super::support::{fixture, reserve, upload_evidence, TestResult};

#[tokio::test]
async fn downstream_retry_resumes_same_attempt_without_repeating_compute() -> TestResult {
    // Given: one remote-completed attempt that fails while downloading.
    let fixture = fixture().await?;
    let attempt_id = reserve(&fixture).await?;
    advance_to_downloading(&fixture, attempt_id).await?;
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
        .fail(
            &task,
            Some(&attempt),
            LifecycleFailure::downstream(DownstreamFailure::Download, "download failed"),
            fixture.now,
        )
        .await?;
    let failed_task = fixture
        .store
        .task(fixture.task_id)
        .await?
        .ok_or_else(|| std::io::Error::other("task missing"))?;
    let failed_attempt = fixture
        .store
        .attempt(attempt_id)
        .await?
        .ok_or_else(|| std::io::Error::other("attempt missing"))?;

    // When: the failed downstream stage is explicitly retried.
    let committed = fixture
        .service
        .retry_downstream(&failed_task, &failed_attempt, fixture.now)
        .await?;

    // Then: the same attempt resumes downloading and no new submission key is created.
    let stored_task = fixture
        .store
        .task(fixture.task_id)
        .await?
        .ok_or_else(|| std::io::Error::other("task missing"))?;
    let attempts = fixture.store.task_attempts(fixture.task_id, 10).await?;
    assert_eq!(committed.action(), DurableAction::Download);
    assert_eq!(stored_task.status, TaskStatus::Downloading);
    assert_eq!(stored_task.attempt_count, 1);
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].attempt.id, attempt_id);
    Ok(())
}

#[tokio::test]
async fn processing_retry_preserves_history_and_requires_new_submission_identity() -> TestResult {
    // Given: a failed processing attempt with terminal remote and cleanup evidence.
    let fixture = fixture().await?;
    let old_attempt_id = reserve(&fixture).await?;
    let remote_job_id = advance_to_processing(&fixture, old_attempt_id).await?;
    let task = fixture
        .store
        .task(fixture.task_id)
        .await?
        .ok_or_else(|| std::io::Error::other("task missing"))?;
    let attempt = fixture
        .store
        .attempt(old_attempt_id)
        .await?
        .ok_or_else(|| std::io::Error::other("attempt missing"))?;
    fixture
        .service
        .fail(
            &task,
            Some(&attempt),
            LifecycleFailure::restart_cancelled("worker restarted"),
            fixture.now,
        )
        .await?;
    let failed_task = fixture
        .store
        .task(fixture.task_id)
        .await?
        .ok_or_else(|| std::io::Error::other("task missing"))?;
    let failed_attempt = fixture
        .store
        .attempt(old_attempt_id)
        .await?
        .ok_or_else(|| std::io::Error::other("attempt missing"))?;
    let old_key = failed_attempt.attempt.submission_key;
    let new_attempt_id = AttemptId::random();
    let new_key = SubmissionKey::random();

    // When: explicit processing retry creates a new reserved attempt.
    let committed = fixture
        .service
        .retry_processing(
            &failed_task,
            &failed_attempt,
            &ProcessingRetryCommand {
                attempt_id: new_attempt_id,
                worker_id: fixture.worker_id,
                submission_key: new_key,
                terminal: TerminalRemoteEvidence::new(
                    remote_job_id,
                    RemoteTerminalStatus::Cancelled,
                ),
                workspace: WorkspaceCleaned::new(fixture.task_id, remote_job_id),
            },
            fixture.now,
        )
        .await?;

    // Then: task paths stay immutable and both old and new attempt identities remain durable.
    let stored_task = fixture
        .store
        .task(fixture.task_id)
        .await?
        .ok_or_else(|| std::io::Error::other("task missing"))?;
    let old_attempt = fixture
        .store
        .attempt(old_attempt_id)
        .await?
        .ok_or_else(|| std::io::Error::other("old attempt missing"))?;
    let new_attempt = fixture
        .store
        .attempt(new_attempt_id)
        .await?
        .ok_or_else(|| std::io::Error::other("new attempt missing"))?;
    assert_eq!(committed.action(), DurableAction::None);
    assert_eq!(stored_task.status, TaskStatus::Reserved);
    assert_eq!(stored_task.attempt_count, 2);
    assert_eq!(
        stored_task.request.input_path.as_str(),
        "/nas/input/episode.v1.mkv"
    );
    assert_eq!(
        stored_task.request.output_path.as_str(),
        "/nas/output/episode.final.mp4"
    );
    assert_eq!(old_attempt.attempt.status, TaskStatus::Failed);
    assert_eq!(old_attempt.attempt.submission_key, old_key);
    assert_eq!(new_attempt.attempt.status, TaskStatus::Reserved);
    assert_eq!(new_attempt.attempt.submission_key, new_key);
    assert_ne!(old_key, new_key);
    Ok(())
}

async fn advance_to_processing(
    fixture: &super::support::Fixture,
    attempt_id: AttemptId,
) -> TestResult<RemoteJobId> {
    let commands = [
        AdvanceCommand::StartUpload,
        AdvanceCommand::FinishUpload(upload_evidence()),
        AdvanceCommand::StartSubmission,
    ];
    for command in commands {
        advance(fixture, attempt_id, command).await?;
    }
    let remote_job_id = RemoteJobId::random();
    advance(
        fixture,
        attempt_id,
        AdvanceCommand::PersistSubmission(videnoa_controller::lifecycle::SubmissionEvidence {
            remote_job_id,
            remote_input_path: videnoa_controller::domain::RemotePath::new("task/input.mkv"),
            remote_output_path: videnoa_controller::domain::RemotePath::new("task/output.mp4"),
        }),
    )
    .await?;
    Ok(remote_job_id)
}

async fn advance_to_downloading(
    fixture: &super::support::Fixture,
    attempt_id: AttemptId,
) -> TestResult {
    advance_to_processing(fixture, attempt_id).await?;
    advance(fixture, attempt_id, AdvanceCommand::FinishProcessing).await?;
    advance(fixture, attempt_id, AdvanceCommand::StartDownload).await
}

async fn advance(
    fixture: &super::support::Fixture,
    attempt_id: AttemptId,
    command: AdvanceCommand,
) -> TestResult {
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
    Ok(())
}
