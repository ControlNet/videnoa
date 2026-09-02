use std::path::PathBuf;

use videnoa_controller::domain::TaskStatus;
use videnoa_controller::scheduler::{DownloadOutcome, PublicationOutcome};

use crate::mock_videnoa::server::MockVidenoa;
use crate::transfer_support::{zero_jitter, Fixture, PreparedTask, TestResult};

pub async fn verified_task(
    server: &MockVidenoa,
    output: &[u8],
) -> TestResult<(Fixture, PreparedTask)> {
    let fixture = Fixture::new(server, 1, 1).await?;
    let prepared = fixture.remote_completed(server, output).await?;
    let outcome = fixture
        .executor()?
        .download(prepared.task_id, fixture.now, zero_jitter()?)
        .await?;
    if !matches!(outcome, DownloadOutcome::Verified(_)) {
        return Err(std::io::Error::other("download did not verify").into());
    }
    Ok((fixture, prepared))
}

pub async fn publish(fixture: &Fixture, prepared: &PreparedTask) -> TestResult<PublicationOutcome> {
    Ok(fixture
        .executor()?
        .publish(prepared.task_id, fixture.now, zero_jitter()?)
        .await?)
}

pub async fn output_path(fixture: &Fixture, prepared: &PreparedTask) -> TestResult<PathBuf> {
    let task = fixture.task(prepared.task_id).await?;
    Ok(PathBuf::from(task.request.output_path.as_str()))
}

pub async fn assert_status(
    fixture: &Fixture,
    prepared: &PreparedTask,
    expected: TaskStatus,
) -> TestResult {
    assert_eq!(fixture.task(prepared.task_id).await?.status, expected);
    Ok(())
}
