use std::path::Path;
use std::time::Duration;

use sha2::{Digest, Sha256};
use videnoa_controller::domain::{Task, TaskStatus};

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
    task: &Task,
) -> TestResult<videnoa_controller::domain::TaskDetailResponse> {
    let completed: TestResult<videnoa_controller::domain::TaskDetailResponse> =
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let detail = fixture.task(task).await?;
                if detail.task.status == TaskStatus::Completed {
                    return Ok(detail);
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .map_err(|_| std::io::Error::other("task did not complete through Controller runtime"))?;
    completed
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
    let detail = wait_for_completed(fixture, task).await?;
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

async fn assert_directory_empty(path: &Path) -> TestResult {
    let mut entries = tokio::fs::read_dir(path).await?;
    assert!(
        entries.next_entry().await?.is_none(),
        "Controller temp root must be empty after successful cleanup"
    );
    Ok(())
}
