use videnoa_controller::scheduler::{DownloadOutcome, UploadOutcome};

use crate::mock_videnoa::checkpoints::Checkpoint;
use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{zero_jitter, Fixture, TestResult};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn saturated_upload_pool_does_not_block_download_pool() -> TestResult {
    // Given: one upload permit is held at the remote acceptance boundary.
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1, 1).await?;
    let downloading = fixture
        .remote_completed(&server, b"download while upload is blocked")
        .await?;
    let uploading = fixture.reserved_task(vec![17_u8; 24_000]).await?;
    let upload_ticket = server.pause(Checkpoint::BeforeAcceptingUpload).await;
    let download_ticket = server.pause(Checkpoint::BeforeDownloadBody).await;
    let upload_executor = fixture.executor()?;
    let upload_now = fixture.now;
    let upload_task = uploading.task_id;
    let upload = tokio::spawn(async move {
        upload_executor
            .upload(
                upload_task,
                upload_now,
                videnoa_controller::lifecycle::JitterSample::try_from(0)?,
            )
            .await
    });
    server.await_checkpoint(&upload_ticket).await?;

    // When: a download starts while the global upload pool is saturated.
    let download_executor = fixture.executor()?;
    let download_now = fixture.now;
    let download_task = downloading.task_id;
    let download = tokio::spawn(async move {
        download_executor
            .download(
                download_task,
                download_now,
                videnoa_controller::lifecycle::JitterSample::try_from(0)?,
            )
            .await
    });

    // Then: it reaches its independent network boundary before upload is released.
    server.await_checkpoint(&download_ticket).await?;
    server.release(download_ticket).await?;
    let download_outcome = download.await??;
    assert!(matches!(download_outcome, DownloadOutcome::Verified(_)));
    server.release(upload_ticket).await?;
    let upload_outcome = upload.await??;
    assert!(matches!(upload_outcome, UploadOutcome::Staged(_)));
    let _ = zero_jitter()?;
    Ok(())
}
