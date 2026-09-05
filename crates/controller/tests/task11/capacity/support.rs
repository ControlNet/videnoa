use videnoa_controller::domain::{AttemptId, SubmissionKey, TaskId, TaskStatus, WorkerId};
use videnoa_controller::persistence::{AttemptRecord, Reservation, ReservationOutcome, TaskRecord};

use super::super::support::{Fixture, TestResult};

pub(super) async fn reserve(fixture: &Fixture, task_id: TaskId, worker_id: WorkerId) -> TestResult {
    let task = fixture
        .store
        .task(task_id)
        .await?
        .ok_or_else(|| std::io::Error::other("task missing before reservation"))?;
    assert!(matches!(
        fixture
            .store
            .reserve_task(&Reservation {
                task_id,
                expected_task_version: task.version,
                worker_id,
                attempt_id: AttemptId::random(),
                submission_key: SubmissionKey::random(),
                reserved_at: fixture.now,
            })
            .await?,
        ReservationOutcome::Reserved(_)
    ));
    Ok(())
}

pub(super) async fn set_status(
    fixture: &Fixture,
    task_id: TaskId,
    status: TaskStatus,
) -> TestResult {
    let status = match status {
        TaskStatus::Queued => "queued",
        TaskStatus::Reserved => "reserved",
        TaskStatus::Uploading => "uploading",
        TaskStatus::Staged => "staged",
        TaskStatus::Submitting => "submitting",
        TaskStatus::Processing => "processing",
        TaskStatus::RemoteCompleted => "remote_completed",
        TaskStatus::Downloading => "downloading",
        TaskStatus::Verifying => "verifying",
        TaskStatus::Publishing => "publishing",
        TaskStatus::RemoteCleanup => "remote_cleanup",
        TaskStatus::Completed => "completed",
        TaskStatus::Failed => "failed",
        TaskStatus::Cancelled => "cancelled",
    };
    let mut transaction = fixture.store.database().pool().begin().await?;
    sqlx::query("UPDATE tasks SET status = ?, version = version + 1 WHERE id = ?")
        .bind(status)
        .bind(task_id.to_string())
        .execute(&mut *transaction)
        .await?;
    sqlx::query("UPDATE task_attempts SET status = ?, version = version + 1 WHERE task_id = ?")
        .bind(status)
        .bind(task_id.to_string())
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(())
}

pub(super) async fn snapshots(
    fixture: &Fixture,
    task_id: TaskId,
) -> TestResult<(TaskRecord, AttemptRecord)> {
    let task = fixture
        .store
        .task(task_id)
        .await?
        .ok_or_else(|| std::io::Error::other("task snapshot missing"))?;
    let attempt = fixture
        .store
        .current_attempt(task_id)
        .await?
        .ok_or_else(|| std::io::Error::other("attempt snapshot missing"))?;
    Ok((task, attempt))
}
