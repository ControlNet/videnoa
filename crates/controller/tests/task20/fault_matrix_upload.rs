use std::time::Duration;

use videnoa_controller::domain::TaskStatus;

use crate::mock_videnoa::checkpoints::Checkpoint;
use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{assert_restarted_pipeline, complete_mock_job, ControllerFixture, TestResult};

pub(crate) async fn restart_mid_upload() -> TestResult {
    let worker = MockVidenoa::start_persistent().await?;
    let mut fixture = ControllerFixture::start().await?;
    fixture.register_worker(&worker, "mid-upload").await?;
    let boundary = worker.pause(Checkpoint::AfterUploadBytesAccepted).await;
    let input = vec![7_u8; 16 * 1024];
    let task = fixture.create_task("mid-upload", &input).await?;

    worker.await_checkpoint(&boundary).await?;
    let accepted = worker.accepted_upload_bytes().await;
    assert!(accepted > 0);
    assert!(accepted < u64::try_from(input.len())?);
    assert_eq!(
        fixture.task(&task).await?.task.status,
        TaskStatus::Uploading
    );

    fixture.crash().await?;
    worker.release(boundary).await?;
    fixture.restart().await?;
    wait_for_remote_job(&worker).await?;
    complete_mock_job(&worker, &task, b"enhanced-video").await?;
    assert_restarted_pipeline(&fixture, &worker, &task, b"enhanced-video").await
}

async fn wait_for_remote_job(worker: &MockVidenoa) -> TestResult {
    tokio::time::timeout(Duration::from_secs(10), async {
        while worker.job_count().await == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| std::io::Error::other("remote job was not created after upload restart"))?;
    Ok(())
}
