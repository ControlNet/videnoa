use std::path::PathBuf;
use std::sync::Arc;

use sha2::{Digest, Sha256};
use videnoa_controller::domain::{Task, TaskStatus};
use videnoa_controller::persistence::Sha256Digest;
use videnoa_controller::scheduler::{TransferCheckpointObserver, TransferCheckpointPoint};

use crate::mock_videnoa::checkpoints::Checkpoint;
use crate::mock_videnoa::journal::Route;
use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{
    assert_restarted_pipeline, complete_mock_job, CheckpointGate, ControllerFixture, TestResult,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn controller_restart_matrix_executes_every_local_boundary() -> TestResult {
    for point in [
        TransferCheckpointPoint::DownloadVerified,
        TransferCheckpointPoint::BeforeDestinationStaging,
        TransferCheckpointPoint::PublicationFinalized,
        TransferCheckpointPoint::BeforeLocalCleanup,
        TransferCheckpointPoint::LocalCleanupCompleted,
        TransferCheckpointPoint::BeforeRemoteDelete,
        TransferCheckpointPoint::RemoteDeleteSucceeded,
    ] {
        eprintln!("task20 crash boundary: {point:?}");
        restart_at_transfer_checkpoint(point).await?;
    }
    Ok(())
}

async fn restart_at_transfer_checkpoint(point: TransferCheckpointPoint) -> TestResult {
    let worker = MockVidenoa::start_persistent().await?;
    let gate = CheckpointGate::new(point);
    let observer: Arc<dyn TransferCheckpointObserver> = gate.clone();
    let mut fixture = ControllerFixture::start_with_checkpoint_observer(Some(observer)).await?;
    fixture
        .register_worker(&worker, &format!("transfer-{point:?}"))
        .await?;
    let run = worker
        .pause(Checkpoint::AfterRunPersistedBeforeResponse)
        .await;
    let task = fixture
        .create_task(&format!("transfer-{point:?}"), b"input-video")
        .await?;
    worker.await_checkpoint(&run).await?;
    complete_mock_job(&worker, &task, b"enhanced-video").await?;
    worker.release(run).await?;
    gate.wait().await?;
    let run_requests = assert_pre_crash_checkpoint(&fixture, &worker, &task, point).await?;
    fixture.crash().await?;
    fixture.restart().await?;
    assert_restarted_pipeline(&fixture, &worker, &task, b"enhanced-video").await?;
    assert_eq!(worker.counters().await.get(Route::Run), run_requests);
    if point == TransferCheckpointPoint::RemoteDeleteSucceeded {
        assert_eq!(worker.counters().await.get(Route::DeleteFile), 2);
    }
    Ok(())
}

async fn assert_pre_crash_checkpoint(
    fixture: &ControllerFixture,
    worker: &MockVidenoa,
    task: &Task,
    point: TransferCheckpointPoint,
) -> TestResult<u64> {
    let record = fixture
        .store
        .task(task.id)
        .await?
        .ok_or_else(|| std::io::Error::other("task record is missing at checkpoint"))?;
    let attempt = fixture
        .store
        .current_attempt(task.id)
        .await?
        .ok_or_else(|| std::io::Error::other("attempt record is missing at checkpoint"))?;
    let expected_status = checkpoint_status(point)?;
    assert_eq!(record.status, expected_status);
    assert_eq!(attempt.attempt.status, expected_status);

    let has_publication_evidence = point != TransferCheckpointPoint::DownloadVerified;
    let expected_sha256 = Sha256Digest::new(Sha256::digest(b"enhanced-video").into());
    assert_eq!(
        record.publication.expected_output_size,
        has_publication_evidence.then_some(14)
    );
    assert_eq!(
        record.publication.expected_output_sha256,
        has_publication_evidence.then_some(expected_sha256)
    );
    assert_eq!(record.publication.destination_staging_name, None);

    let temporary = fixture.temp_root.join(task.id.to_string());
    let verified = temporary.join("output.mp4.verified");
    let evidence = temporary.join("output.mp4.verified.evidence");
    let part = temporary.join("output.mp4.part");
    let output = PathBuf::from(task.output_path.as_str());
    let has_verified_source = matches!(
        point,
        TransferCheckpointPoint::DownloadVerified
            | TransferCheckpointPoint::BeforeDestinationStaging
    );
    let keeps_temporary = matches!(
        point,
        TransferCheckpointPoint::DownloadVerified
            | TransferCheckpointPoint::BeforeDestinationStaging
            | TransferCheckpointPoint::PublicationFinalized
            | TransferCheckpointPoint::BeforeLocalCleanup
    );
    let has_final = matches!(
        point,
        TransferCheckpointPoint::PublicationFinalized
            | TransferCheckpointPoint::BeforeLocalCleanup
            | TransferCheckpointPoint::LocalCleanupCompleted
            | TransferCheckpointPoint::BeforeRemoteDelete
            | TransferCheckpointPoint::RemoteDeleteSucceeded
    );
    assert!(
        !part.exists(),
        "partial download must never survive a checkpoint"
    );
    assert_eq!(verified.exists(), has_verified_source);
    assert_eq!(evidence.exists(), keeps_temporary);
    assert_eq!(output.exists(), has_final);
    assert_eq!(temporary.exists(), keeps_temporary);
    assert_eq!(
        worker.file_count().await,
        if point == TransferCheckpointPoint::RemoteDeleteSucceeded {
            0
        } else {
            2
        }
    );
    let run_requests = worker.counters().await.get(Route::Run);
    assert_eq!(run_requests, 1);
    Ok(run_requests)
}

fn checkpoint_status(point: TransferCheckpointPoint) -> TestResult<TaskStatus> {
    Ok(match point {
        TransferCheckpointPoint::DownloadVerified => TaskStatus::Downloading,
        TransferCheckpointPoint::BeforeDestinationStaging
        | TransferCheckpointPoint::PublicationFinalized => TaskStatus::Publishing,
        TransferCheckpointPoint::BeforeLocalCleanup
        | TransferCheckpointPoint::LocalCleanupCompleted
        | TransferCheckpointPoint::BeforeRemoteDelete
        | TransferCheckpointPoint::RemoteDeleteSucceeded => TaskStatus::RemoteCleanup,
        TransferCheckpointPoint::UploadCompleted
        | TransferCheckpointPoint::BeforeRemoteSubmit
        | TransferCheckpointPoint::RemoteCompletionPersisted
        | TransferCheckpointPoint::DestinationStaged
        | TransferCheckpointPoint::StagingVerified => {
            return Err(
                std::io::Error::other("checkpoint is not a local recovery boundary").into(),
            );
        }
    })
}
