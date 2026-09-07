use std::path::Path;
use std::time::Duration;

use sha2::{Digest, Sha256};
use videnoa_controller::domain::{Task, TaskStatus};
use videnoa_controller::persistence::{AttemptRecord, TaskRecord};

use crate::mock_videnoa::journal::Route;
use crate::mock_videnoa::server::MockVidenoa;

use super::{ControllerFixture, TestResult};

const FIRST_JOB_ID: &str = "00000000-0000-4000-8000-000000000001";

pub async fn complete_mock_job(server: &MockVidenoa, task: &Task, output: &[u8]) -> TestResult {
    tokio::time::timeout(Duration::from_secs(10), async {
        while server.job_count().await == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| std::io::Error::other("remote job was not persisted"))?;
    server
        .complete_job(FIRST_JOB_ID, &format!("{}/output.mp4", task.id), output)
        .await?;
    Ok(())
}

pub async fn wait_for_completed(
    fixture: &ControllerFixture,
    server: &MockVidenoa,
    task: &Task,
) -> TestResult<videnoa_controller::domain::TaskDetailResponse> {
    match tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let detail = fixture.task(task).await?;
            if detail.task.status == TaskStatus::Completed {
                return Ok(detail);
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    {
        Ok(completed) => completed,
        Err(_) => Err(completion_timeout(fixture, server, task).await.into()),
    }
}

pub async fn wait_for_positive_download_partial(
    fixture: &ControllerFixture,
    server: &MockVidenoa,
    task: &Task,
) -> TestResult {
    let part = fixture
        .temp_root
        .join(task.id.to_string())
        .join("output.mp4.part");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut last_part_state = String::from("missing");
    loop {
        match tokio::fs::metadata(&part).await {
            Ok(metadata) => {
                last_part_state = format!("present length={}", metadata.len());
                if metadata.len() > 0 {
                    return Ok(());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                last_part_state = format!("metadata error={error}");
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(std::io::Error::other(format!(
                "download partial did not become positive: task_id={} part={} part_state={} {}",
                task.id,
                part.display(),
                last_part_state,
                diagnostic_snapshot(fixture, server, task).await
            ))
            .into());
        }
        tokio::task::yield_now().await;
    }
}

pub async fn coherent_task_attempt(
    fixture: &ControllerFixture,
    task: &Task,
    expected_status: TaskStatus,
    operation: &str,
) -> TestResult<(TaskRecord, AttemptRecord)> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let task_before = fixture.store.task(task.id).await?;
        let attempt = fixture.store.current_attempt(task.id).await?;
        let task_after = fixture.store.task(task.id).await?;
        let snapshot =
            format!("task_before={task_before:?} attempt={attempt:?} task_after={task_after:?}");
        if let (Some(task_before), Some(attempt), Some(task_after)) =
            (task_before, attempt, task_after)
        {
            let task_is_stable = task_before.version == task_after.version
                && task_before.status == task_after.status;
            let pair_matches = task_after.id == attempt.attempt.task_id
                && task_after.status == attempt.attempt.status;
            if task_is_stable && pair_matches && task_after.status == expected_status {
                return Ok((task_after, attempt));
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(std::io::Error::other(format!(
                "{operation} could not acquire coherent {expected_status:?} lifecycle snapshots: {snapshot}"
            ))
            .into());
        }
        tokio::task::yield_now().await;
    }
}

pub fn lifecycle_operation_error(
    operation: &str,
    task: &TaskRecord,
    attempt: &AttemptRecord,
    error: impl std::fmt::Display,
) -> std::io::Error {
    std::io::Error::other(format!(
        "{operation} failed: task_id={} task_version={} task_status={:?} attempt_id={} attempt_version={} attempt_status={:?}: {error}",
        task.id,
        task.version,
        task.status,
        attempt.attempt.id,
        attempt.version,
        attempt.attempt.status
    ))
}

pub async fn assert_completed_pipeline(
    fixture: &ControllerFixture,
    server: &MockVidenoa,
    task: &Task,
    expected: &[u8],
) -> TestResult {
    assert_pipeline(fixture, server, task, expected, Some(1)).await
}

pub async fn assert_restarted_pipeline(
    fixture: &ControllerFixture,
    server: &MockVidenoa,
    task: &Task,
    expected: &[u8],
) -> TestResult {
    assert_pipeline(fixture, server, task, expected, None).await
}

async fn assert_pipeline(
    fixture: &ControllerFixture,
    server: &MockVidenoa,
    task: &Task,
    expected: &[u8],
    exact_run_requests: Option<u64>,
) -> TestResult {
    let detail = wait_for_completed(fixture, server, task).await?;
    assert_eq!(detail.attempts.len(), 1, "attempt history must be retained");
    assert_eq!(detail.task.attempt_count, 1, "compute must not replay");
    let output = tokio::fs::read(detail.task.output_path.as_str()).await?;
    assert_eq!(output, expected, "published bytes must match mock output");
    assert_eq!(Sha256::digest(&output), Sha256::digest(expected));
    assert_directory_empty(&fixture.temp_root).await?;
    let counters = server.counters().await;
    match exact_run_requests {
        Some(expected) => assert_eq!(counters.get(Route::Run), expected),
        None => assert!(counters.get(Route::Run) >= 1),
    }
    assert!(counters.get(Route::Upload) >= 1);
    assert!(counters.get(Route::JobPoll) >= 1);
    assert!(counters.get(Route::Download) >= 1);
    assert!(counters.get(Route::DeleteFile) >= 1);
    assert_eq!(server.job_count().await, 1);
    assert_eq!(server.file_count().await, 0);
    Ok(())
}

async fn completion_timeout(
    fixture: &ControllerFixture,
    server: &MockVidenoa,
    task: &Task,
) -> std::io::Error {
    std::io::Error::other(format!(
        "task did not complete through Controller runtime: task_id={} {}",
        task.id,
        diagnostic_snapshot(fixture, server, task).await
    ))
}

async fn diagnostic_snapshot(
    fixture: &ControllerFixture,
    server: &MockVidenoa,
    task: &Task,
) -> String {
    let durable_task = fixture.store.task(task.id).await;
    let durable_attempt = fixture.store.current_attempt(task.id).await;
    let counters = server.counters().await;
    let journal = server.journal().await;
    format!(
        "durable_task={durable_task:?} durable_attempt={durable_attempt:?} remote_counts={{run:{},poll:{},stat:{},download:{},delete:{},jobs:{},files:{}}} last_remote_event={:?}",
        counters.get(Route::Run),
        counters.get(Route::JobPoll),
        counters.get(Route::Stat),
        counters.get(Route::Download),
        counters.get(Route::DeleteFile),
        server.job_count().await,
        server.file_count().await,
        journal.last()
    )
}

async fn assert_directory_empty(path: &Path) -> TestResult {
    let mut entries = tokio::fs::read_dir(path).await?;
    assert!(
        entries.next_entry().await?.is_none(),
        "Controller temp root must be empty after successful cleanup"
    );
    Ok(())
}
