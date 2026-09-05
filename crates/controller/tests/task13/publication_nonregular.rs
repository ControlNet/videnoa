use std::path::{Path, PathBuf};

use videnoa_controller::domain::FailureCode;
use videnoa_controller::lifecycle::{AdvanceCommand, LifecycleService, PublicationIntent};
use videnoa_controller::scheduler::PublicationOutcome;

use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{output_path, publish, verified_task};
use crate::transfer_support::{verified_path, Fixture, PreparedTask, TestResult};

#[cfg(target_os = "linux")]
use std::os::unix::fs::FileTypeExt;

#[cfg(target_os = "linux")]
#[tokio::test]
async fn final_fifo_is_ambiguous_without_blocking_or_mutation() -> TestResult {
    // Given: a FIFO occupies the exact final leaf before direct publication recovery.
    let server = MockVidenoa::start().await?;
    let output = b"final fifo safety".repeat(1024);
    let (fixture, prepared, destination) =
        publishing_task(&server, &output, PublicationIntent::direct()).await?;
    create_fifo(&destination)?;

    // When: recovery inspects the final leaf through a non-blocking no-follow capability.
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        publish(&fixture, &prepared),
    )
    .await??;

    // Then: the FIFO remains intact and publication terminates as ambiguity.
    assert_eq!(outcome, PublicationOutcome::Failed);
    assert!(std::fs::symlink_metadata(&destination)?
        .file_type()
        .is_fifo());
    assert_ambiguous(&fixture, &prepared).await?;
    Ok(())
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn final_symlink_is_ambiguous_without_touching_its_target() -> TestResult {
    // Given: the exact final leaf is a symlink to an unrelated sentinel file.
    let server = MockVidenoa::start().await?;
    let output = b"final symlink safety".repeat(1024);
    let (fixture, prepared, destination) =
        publishing_task(&server, &output, PublicationIntent::direct()).await?;
    let sentinel = fixture.output_root.join("final-symlink-sentinel");
    tokio::fs::write(&sentinel, b"sentinel bytes").await?;
    std::os::unix::fs::symlink(&sentinel, &destination)?;

    // When: recovery inspects the final leaf without following it.
    let outcome = publish(&fixture, &prepared).await?;

    // Then: both symlink and target remain unchanged and ownership is ambiguous.
    assert_eq!(outcome, PublicationOutcome::Failed);
    assert!(std::fs::symlink_metadata(&destination)?
        .file_type()
        .is_symlink());
    assert_eq!(tokio::fs::read(&sentinel).await?, b"sentinel bytes");
    assert_ambiguous(&fixture, &prepared).await?;
    Ok(())
}

#[tokio::test]
async fn final_directory_is_ambiguous_without_mutation() -> TestResult {
    // Given: a directory occupies the exact final publication leaf.
    let server = MockVidenoa::start().await?;
    let output = b"final directory safety".repeat(1024);
    let (fixture, prepared, destination) =
        publishing_task(&server, &output, PublicationIntent::direct()).await?;
    tokio::fs::create_dir(&destination).await?;

    // When: direct recovery classifies the non-regular final node.
    let outcome = publish(&fixture, &prepared).await?;

    // Then: the directory remains and publication terminates as ambiguity.
    assert_eq!(outcome, PublicationOutcome::Failed);
    assert!(destination.is_dir());
    assert_ambiguous(&fixture, &prepared).await?;
    Ok(())
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn legacy_staging_fifo_is_ambiguous_without_blocking_or_mutation() -> TestResult {
    // Given: a legacy row names a FIFO under the output parent.
    let server = MockVidenoa::start().await?;
    let output = b"legacy staging fifo safety".repeat(1024);
    let staging_name = ".videnoa-legacy-fifo.staging";
    let (fixture, prepared, destination) =
        publishing_task(&server, &output, PublicationIntent::new(staging_name)).await?;
    let staging = sibling(&destination, staging_name)?;
    create_fifo(&staging)?;

    // When: legacy recovery inspects the named sibling without blocking.
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        publish(&fixture, &prepared),
    )
    .await??;

    // Then: the FIFO is preserved and the legacy evidence is ambiguous.
    assert_eq!(outcome, PublicationOutcome::Failed);
    assert!(std::fs::symlink_metadata(&staging)?.file_type().is_fifo());
    assert_ambiguous(&fixture, &prepared).await?;
    Ok(())
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn legacy_staging_symlink_is_ambiguous_without_touching_its_target() -> TestResult {
    // Given: a legacy row names a symlink to an unrelated sentinel file.
    let server = MockVidenoa::start().await?;
    let output = b"legacy staging symlink safety".repeat(1024);
    let staging_name = ".videnoa-legacy-symlink.staging";
    let (fixture, prepared, destination) =
        publishing_task(&server, &output, PublicationIntent::new(staging_name)).await?;
    let staging = sibling(&destination, staging_name)?;
    let sentinel = fixture.output_root.join("legacy-symlink-sentinel");
    tokio::fs::write(&sentinel, b"sentinel bytes").await?;
    std::os::unix::fs::symlink(&sentinel, &staging)?;

    // When: legacy recovery inspects the named sibling without following it.
    let outcome = publish(&fixture, &prepared).await?;

    // Then: symlink and target remain unchanged and the state is ambiguous.
    assert_eq!(outcome, PublicationOutcome::Failed);
    assert!(std::fs::symlink_metadata(&staging)?
        .file_type()
        .is_symlink());
    assert_eq!(tokio::fs::read(&sentinel).await?, b"sentinel bytes");
    assert_ambiguous(&fixture, &prepared).await?;
    Ok(())
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn verified_temp_fifo_is_ambiguous_without_blocking_or_mutation() -> TestResult {
    // Given: the verified temp leaf was replaced by a FIFO before direct recovery.
    let server = MockVidenoa::start().await?;
    let output = b"verified temp fifo safety".repeat(1024);
    let (fixture, prepared, destination) =
        publishing_task(&server, &output, PublicationIntent::direct()).await?;
    let verified = verified_path(&fixture.temp_root, prepared.task_id);
    tokio::fs::remove_file(&verified).await?;
    create_fifo(&verified)?;

    // When: recovery inspects the temp leaf through a non-blocking no-follow capability.
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        publish(&fixture, &prepared),
    )
    .await??;

    // Then: the FIFO remains under temp_root, no final appears, and the state is ambiguous.
    assert_eq!(outcome, PublicationOutcome::Failed);
    assert!(std::fs::symlink_metadata(&verified)?.file_type().is_fifo());
    assert!(!destination.exists());
    assert_ambiguous(&fixture, &prepared).await?;
    Ok(())
}

async fn publishing_task(
    server: &MockVidenoa,
    output: &[u8],
    intent: PublicationIntent,
) -> TestResult<(Fixture, PreparedTask, PathBuf)> {
    let (fixture, prepared) = verified_task(server, output).await?;
    let task = fixture.task(prepared.task_id).await?;
    let attempt = fixture.attempt(prepared.attempt_id).await?;
    LifecycleService::new(fixture.store.clone())
        .advance(
            &task,
            &attempt,
            AdvanceCommand::FinishVerification(intent),
            fixture.now,
        )
        .await?;
    let destination = output_path(&fixture, &prepared).await?;
    Ok((fixture, prepared, destination))
}

async fn assert_ambiguous(fixture: &Fixture, prepared: &PreparedTask) -> TestResult {
    assert_eq!(
        fixture
            .task(prepared.task_id)
            .await?
            .failure
            .map(|failure| failure.failure_code),
        Some(FailureCode::PublicationAmbiguous)
    );
    Ok(())
}

fn sibling(destination: &Path, name: &str) -> TestResult<PathBuf> {
    Ok(destination
        .parent()
        .ok_or_else(|| std::io::Error::other("destination parent missing"))?
        .join(name))
}

#[cfg(target_os = "linux")]
fn create_fifo(path: &Path) -> TestResult {
    rustix::fs::mknodat(
        rustix::fs::CWD,
        path,
        rustix::fs::FileType::Fifo,
        rustix::fs::Mode::RWXU,
        rustix::fs::makedev(0, 0),
    )?;
    Ok(())
}
