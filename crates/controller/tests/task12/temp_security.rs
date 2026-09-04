use std::time::Duration;

use videnoa_controller::scheduler::DownloadOutcome;

use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{evidence_path, part_path, verified_path, zero_jitter, Fixture, TestResult};

const SENTINEL: &[u8] = b"outside sentinel must remain unchanged";

#[tokio::test]
async fn normal_download_writes_recoverable_verified_evidence() -> TestResult {
    // Given: a remotely completed task with no local artifacts.
    let mut server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1, 1).await?;
    let output = b"capability baseline output".repeat(1024);
    let prepared = fixture.remote_completed(&server, &output).await?;

    // When: download completes and a second offline execution recovers it.
    let first = fixture
        .executor()?
        .download(prepared.task_id, fixture.now, zero_jitter()?)
        .await?;
    restore_downloading(&fixture, prepared.task_id, prepared.attempt_id).await?;
    server
        .set_offline(crate::mock_videnoa::faults::OfflineMode::ConnectionRefused)
        .await?;
    let second = fixture
        .executor()?
        .download(prepared.task_id, fixture.now, zero_jitter()?)
        .await?;

    // Then: exact verified bytes and the fixed-size sidecar survive restart recovery.
    assert!(matches!(first, DownloadOutcome::Verified(_)));
    assert!(matches!(second, DownloadOutcome::Verified(_)));
    assert_eq!(
        tokio::fs::read(verified_path(&fixture.temp_root, prepared.task_id)).await?,
        output
    );
    assert_eq!(
        tokio::fs::metadata(evidence_path(&fixture.temp_root, prepared.task_id))
            .await?
            .len(),
        40
    );
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn part_leaf_symlink_never_modifies_outside_sentinel() -> TestResult {
    // Given: the owned part leaf is a symlink to an outside regular file.
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1, 1).await?;
    let output = b"hostile part replacement".repeat(1024);
    let prepared = fixture.remote_completed(&server, &output).await?;
    let outside = fixture.directory.path().join("outside-part");
    tokio::fs::write(&outside, SENTINEL).await?;
    let part = part_path(&fixture.temp_root, prepared.task_id);
    tokio::fs::create_dir_all(parent(&part)?).await?;
    std::os::unix::fs::symlink(&outside, &part)?;

    // When: the production download attempts to create/truncate the part artifact.
    let outcome = fixture
        .executor()?
        .download(prepared.task_id, fixture.now, zero_jitter()?)
        .await?;

    // Then: the attack fails closed without touching the outside file.
    assert!(matches!(outcome, DownloadOutcome::RetryScheduled { .. }));
    assert_eq!(tokio::fs::read(outside).await?, SENTINEL);
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn evidence_leaf_symlink_never_modifies_outside_sentinel() -> TestResult {
    // Given: the durable evidence leaf is a symlink to an outside regular file.
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1, 1).await?;
    let output = b"hostile evidence replacement".repeat(1024);
    let prepared = fixture.remote_completed(&server, &output).await?;
    let outside = fixture.directory.path().join("outside-evidence");
    tokio::fs::write(&outside, SENTINEL).await?;
    let evidence = evidence_path(&fixture.temp_root, prepared.task_id);
    tokio::fs::create_dir_all(parent(&evidence)?).await?;
    std::os::unix::fs::symlink(&outside, &evidence)?;

    // When: the production download attempts to persist evidence.
    let outcome = fixture
        .executor()?
        .download(prepared.task_id, fixture.now, zero_jitter()?)
        .await?;

    // Then: the attack fails closed without truncating the outside file.
    assert!(matches!(outcome, DownloadOutcome::RetryScheduled { .. }));
    assert_eq!(tokio::fs::read(outside).await?, SENTINEL);
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn task_directory_symlink_never_creates_outside_artifacts() -> TestResult {
    // Given: the task workspace pathname is a symlink to an outside directory.
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1, 1).await?;
    let output = b"hostile task directory".repeat(1024);
    let prepared = fixture.remote_completed(&server, &output).await?;
    let outside = fixture.directory.path().join("outside-task");
    tokio::fs::create_dir(&outside).await?;
    tokio::fs::write(outside.join("sentinel"), SENTINEL).await?;
    std::os::unix::fs::symlink(
        &outside,
        fixture.temp_root.join(prepared.task_id.to_string()),
    )?;

    // When: download tries to create the task workspace and its artifacts.
    let outcome = fixture
        .executor()?
        .download(prepared.task_id, fixture.now, zero_jitter()?)
        .await?;

    // Then: no artifact is created through the substituted directory.
    assert!(matches!(outcome, DownloadOutcome::RetryScheduled { .. }));
    assert_eq!(tokio::fs::read(outside.join("sentinel")).await?, SENTINEL);
    assert!(!outside.join("output.mp4.part").exists());
    assert!(!outside.join("output.mp4.verified").exists());
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn part_fifo_is_rejected_without_blocking() -> TestResult {
    // Given: the part leaf is a FIFO with no reader.
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1, 1).await?;
    let output = b"fifo replacement".repeat(1024);
    let prepared = fixture.remote_completed(&server, &output).await?;
    let part = part_path(&fixture.temp_root, prepared.task_id);
    tokio::fs::create_dir_all(parent(&part)?).await?;
    let status = std::process::Command::new("mkfifo").arg(&part).status()?;
    if !status.success() {
        return Err(std::io::Error::other("mkfifo failed").into());
    }

    // When: download reaches the hostile leaf.
    let result = tokio::time::timeout(
        Duration::from_secs(1),
        fixture
            .executor()?
            .download(prepared.task_id, fixture.now, zero_jitter()?),
    )
    .await;

    // Then: node classification completes without waiting for a FIFO peer.
    let outcome = result.map_err(|_| std::io::Error::other("download blocked on FIFO"))??;
    assert!(matches!(outcome, DownloadOutcome::RetryScheduled { .. }));
    Ok(())
}

async fn restore_downloading(
    fixture: &Fixture,
    task_id: videnoa_controller::domain::TaskId,
    attempt_id: videnoa_controller::domain::AttemptId,
) -> TestResult {
    sqlx::query(
        "UPDATE tasks SET status = 'downloading', expected_output_size = NULL,
            expected_output_sha256 = NULL WHERE id = ?",
    )
    .bind(task_id.to_string())
    .execute(fixture.store.database().pool())
    .await?;
    sqlx::query("UPDATE task_attempts SET status = 'downloading' WHERE id = ?")
        .bind(attempt_id.to_string())
        .execute(fixture.store.database().pool())
        .await?;
    Ok(())
}

fn parent(path: &std::path::Path) -> TestResult<&std::path::Path> {
    path.parent()
        .ok_or_else(|| std::io::Error::other("artifact parent missing").into())
}
