use std::error::Error;

use reqwest::StatusCode;
use serde_json::json;

use crate::mock_videnoa::api::MockClient;
use crate::mock_videnoa::checkpoints::Checkpoint;
use crate::mock_videnoa::domain::{JobProgress, JobStatus};
use crate::mock_videnoa::journal::Route;
use crate::mock_videnoa::server::MockVidenoa;

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn complete_remote_lifecycle_preserves_contract_bytes_and_journal() -> TestResult {
    // Given: a real TCP mock and an upload paused before request acceptance.
    let server = MockVidenoa::start().await?;
    let client = MockClient::new(server.base_url())?;
    assert_eq!(client.health().await?.status, "ok");
    let catalog = client.catalog().await?;
    assert!(catalog.contains_eligible("eligible-workflow.json"));
    assert!(catalog.contains_eligible("eligible-preset"));
    assert!(catalog.contains_incompatible("missing-path.json"));
    assert!(catalog.contains_incompatible("wrong-path-type.json"));

    let ticket = server.pause(Checkpoint::BeforeAcceptingUpload).await;
    let upload_client = client.clone();
    let upload = tokio::spawn(async move {
        upload_client
            .upload("task-001/input.mkv", b"input-video-bytes")
            .await
    });
    server.await_checkpoint(&ticket).await?;
    assert!(!upload.is_finished());
    server.release(ticket).await?;

    // When: the upload, keyed run, controlled progress, output, stat, and cleanup complete.
    let uploaded = upload.await??;
    assert_eq!(uploaded.size, 17);
    assert_eq!(uploaded.path, "../mock-worker/workspace/task-001/input.mkv");
    let created = client
        .run(
            "eligible-workflow.json",
            "happy-key",
            json!({"input": uploaded.path, "output": "../mock-worker/workspace/task-001/output.mp4"}),
        )
        .await?;
    assert_eq!(created.status, StatusCode::CREATED);
    server
        .set_job(
            &created.body.id,
            JobStatus::Running,
            Some(JobProgress::new(7, Some(20), 3.5, Some(4.0))),
        )
        .await?;
    assert_eq!(
        client.job(&created.body.id).await?.status,
        JobStatus::Running
    );
    server
        .complete_job(
            &created.body.id,
            "task-001/output.mp4",
            b"enhanced-output-bytes",
        )
        .await?;
    assert_eq!(
        client.job(&created.body.id).await?.status,
        JobStatus::Completed
    );
    assert_eq!(
        client.download("task-001/output.mp4").await?,
        b"enhanced-output-bytes"
    );
    let stat = client.stat("task-001/output.mp4").await?;
    assert_eq!(stat.size, 21);
    assert!(stat.is_file);
    assert!(!stat.is_dir);
    assert_eq!(client.delete("task-001").await?, StatusCode::NO_CONTENT);

    // Then: accepted requests retain exact wire evidence in deterministic order.
    let counters = server.counters().await;
    assert_eq!(counters.get(Route::Upload), 1);
    assert_eq!(counters.get(Route::Run), 1);
    assert_eq!(counters.get(Route::JobPoll), 2);
    assert_eq!(counters.get(Route::Download), 1);
    assert_eq!(counters.get(Route::Stat), 1);
    assert_eq!(counters.get(Route::DeleteFile), 1);
    assert_eq!(server.active_job_count().await, 0);
    assert_eq!(server.peak_active_jobs().await, 1);
    let journal = server.journal().await;
    assert!(journal
        .windows(2)
        .all(|pair| pair[0].sequence < pair[1].sequence));
    let upload_entry = journal
        .iter()
        .find(|entry| entry.route == Route::Upload)
        .ok_or_else(|| std::io::Error::other("upload journal entry missing"))?;
    assert_eq!(upload_entry.method, "PUT");
    assert_eq!(upload_entry.path, "/api/files/task-001/input.mkv");
    assert_eq!(upload_entry.body, b"input-video-bytes");
    assert!(upload_entry
        .checkpoints
        .contains_key("before_accepting_upload"));
    server.write_happy_evidence_if_requested().await?;
    Ok(())
}
