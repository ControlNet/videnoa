use videnoa_controller::domain::TaskStatus;
use videnoa_controller::scheduler::{PublicationOutcome, TransferCheckpointPoint};

use crate::checkpoints::CheckpointGate;
use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{publish, verified_task};
use crate::transfer_support::{zero_jitter, TestResult};

const SENTINEL: &[u8] = b"cleanup outside sentinel";

#[cfg(unix)]
#[tokio::test]
async fn configured_temp_root_replacement_never_deletes_outside_tree() -> TestResult {
    // Given: publication pauses before cleanup and the configured root is replaced by a symlink.
    let server = MockVidenoa::start().await?;
    let output = b"root replacement cleanup".repeat(1024);
    let (fixture, prepared) = verified_task(&server, &output).await?;
    let gate = CheckpointGate::new(TransferCheckpointPoint::BeforeLocalCleanup);
    let executor = fixture.executor()?.with_checkpoint_observer(gate.clone());
    let task_id = prepared.task_id;
    let now = fixture.now;
    let operation = tokio::spawn(async move {
        let outcome = executor.publish(task_id, now, zero_jitter()?).await?;
        TestResult::Ok(outcome)
    });
    gate.wait().await?;
    let retained = fixture.directory.path().join("retained-temp");
    tokio::fs::rename(&fixture.temp_root, &retained).await?;
    let outside = fixture.directory.path().join("outside-cleanup-root");
    let outside_task = outside.join(prepared.task_id.to_string());
    tokio::fs::create_dir_all(&outside_task).await?;
    tokio::fs::write(outside_task.join("sentinel"), SENTINEL).await?;
    std::os::unix::fs::symlink(&outside, &fixture.temp_root)?;

    // When: local cleanup resumes through the configured pathname.
    gate.release();
    let outcome = operation.await??;
    let sentinel_survived = tokio::fs::read(outside_task.join("sentinel")).await.ok();
    tokio::fs::remove_file(&fixture.temp_root).await?;
    tokio::fs::rename(&retained, &fixture.temp_root).await?;

    // Then: cleanup retries and the outside tree remains byte-identical.
    assert!(matches!(outcome, PublicationOutcome::RetryScheduled { .. }));
    assert_eq!(sentinel_survived.as_deref(), Some(SENTINEL));
    assert_eq!(
        fixture.task(prepared.task_id).await?.status,
        TaskStatus::RemoteCleanup
    );
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn task_directory_replacement_never_removes_substituted_node() -> TestResult {
    // Given: publication pauses before cleanup and its task directory is substituted.
    let server = MockVidenoa::start().await?;
    let output = b"task replacement cleanup".repeat(1024);
    let (fixture, prepared) = verified_task(&server, &output).await?;
    let gate = CheckpointGate::new(TransferCheckpointPoint::BeforeLocalCleanup);
    let executor = fixture.executor()?.with_checkpoint_observer(gate.clone());
    let task_id = prepared.task_id;
    let now = fixture.now;
    let operation = tokio::spawn(async move {
        let outcome = executor.publish(task_id, now, zero_jitter()?).await?;
        TestResult::Ok(outcome)
    });
    gate.wait().await?;
    let workspace = fixture.temp_root.join(prepared.task_id.to_string());
    let retained = fixture.temp_root.join("retained-task");
    tokio::fs::rename(&workspace, &retained).await?;
    let outside = fixture.directory.path().join("outside-cleanup-task");
    tokio::fs::create_dir(&outside).await?;
    tokio::fs::write(outside.join("sentinel"), SENTINEL).await?;
    std::os::unix::fs::symlink(&outside, &workspace)?;

    // When: local cleanup resumes after task identity changed.
    gate.release();
    let outcome = operation.await??;

    // Then: cleanup fails closed without removing the substitute or outside file.
    assert!(matches!(outcome, PublicationOutcome::RetryScheduled { .. }));
    assert!(workspace.symlink_metadata()?.file_type().is_symlink());
    assert_eq!(tokio::fs::read(outside.join("sentinel")).await?, SENTINEL);
    assert!(retained.exists());
    Ok(())
}

#[tokio::test]
async fn normal_local_cleanup_removes_only_the_owned_task_workspace() -> TestResult {
    // Given: a verified task and an unrelated sibling in the Controller temp root.
    let server = MockVidenoa::start().await?;
    let output = b"normal cleanup baseline".repeat(1024);
    let (fixture, prepared) = verified_task(&server, &output).await?;
    let sibling = fixture.temp_root.join("unrelated-sibling");
    tokio::fs::write(&sibling, SENTINEL).await?;

    // When: publication and cleanup converge normally.
    let outcome = publish(&fixture, &prepared).await?;

    // Then: only the task workspace is removed.
    assert_eq!(outcome, PublicationOutcome::Completed);
    assert!(!fixture
        .temp_root
        .join(prepared.task_id.to_string())
        .exists());
    assert_eq!(tokio::fs::read(sibling).await?, SENTINEL);
    Ok(())
}
