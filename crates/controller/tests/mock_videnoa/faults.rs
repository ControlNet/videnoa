use std::error::Error;

use reqwest::StatusCode;
use serde_json::json;

use crate::mock_videnoa::checkpoints::Checkpoint;
use crate::mock_videnoa::client::MockClient;
use crate::mock_videnoa::faults::{DeleteOutcome, Fault, OfflineMode, ResponseFault};
use crate::mock_videnoa::journal::Route;
use crate::mock_videnoa::server::MockVidenoa;

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn transport_faults_are_real_and_preserve_acceptance_boundaries() -> TestResult {
    // Given: a TCP mock with a disconnect armed before upload acceptance.
    let server = MockVidenoa::start().await?;
    let client = MockClient::new(server.base_url())?;
    server.set_fault(Fault::DisconnectBeforeAccept).await;

    // When: upload hits the real connection and a keyed run loses its response after persistence.
    let disconnect = client.upload("fault/input.mkv", b"unaccepted").await;
    assert!(disconnect.is_err());
    assert_eq!(server.counters().await.get(Route::Upload), 0);
    server.set_fault(Fault::AcceptThenDropRunResponse).await;
    let dropped = client
        .run(
            "eligible-workflow.json",
            "drop-key",
            json!({"input": "fault/input.mkv"}),
        )
        .await;

    // Then: the response is a transport error, but replay recovers one persisted job.
    assert!(dropped.is_err());
    assert_eq!(server.job_count().await, 1);
    let replay = client
        .run(
            "eligible-workflow.json",
            "drop-key",
            json!({"input": "fault/input.mkv"}),
        )
        .await?;
    assert_eq!(replay.status, StatusCode::OK);
    assert_eq!(server.job_count().await, 1);
    Ok(())
}

#[tokio::test]
async fn delayed_poll_waits_for_named_release_without_sleep() -> TestResult {
    // Given: a job and a poll-response checkpoint armed before the request.
    let server = MockVidenoa::start().await?;
    let client = MockClient::new(server.base_url())?;
    let created = client
        .run("eligible-workflow.json", "poll-key", json!({}))
        .await?;
    let ticket = server.pause(Checkpoint::BeforePollResponse).await;
    let poll_client = client.clone();
    let job_id = created.body.id;
    let poll = tokio::spawn(async move { poll_client.job(&job_id).await });

    // When: the checkpoint is observed before explicit release.
    server.await_checkpoint(&ticket).await?;
    assert!(!poll.is_finished());
    server.release(ticket).await?;

    // Then: the exact blocked poll completes after release.
    assert!(poll.await??.status.is_active());
    Ok(())
}

#[tokio::test]
async fn offline_truncated_and_corrupt_modes_have_distinct_client_outcomes() -> TestResult {
    // Given: one stored output and a live mock URL.
    let mut server = MockVidenoa::start().await?;
    let client = MockClient::new(server.base_url())?;
    server
        .store_file("fault/output.mp4", b"expected-output")
        .await?;

    // When/Then: HTTP-offline returns 503 and connection-offline refuses the same URL.
    server.set_offline(OfflineMode::ServiceUnavailable).await?;
    assert_eq!(
        client.health_raw().await?.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    server.resume().await?;
    server.set_offline(OfflineMode::ConnectionRefused).await?;
    assert!(client.health_raw().await.is_err());
    server.resume().await?;

    // When/Then: truncated output fails body reading while corrupt output completes distinctly.
    server
        .set_fault(Fault::TruncateDownload { delivered_bytes: 4 })
        .await;
    let truncated = client.download("fault/output.mp4").await;
    assert!(truncated.is_err());
    server
        .set_fault(Fault::CorruptOutput {
            bytes: b"complete-but-corrupt".to_vec(),
        })
        .await;
    let corrupt = client.download("fault/output.mp4").await?;
    assert_eq!(corrupt, b"complete-but-corrupt");
    assert_ne!(corrupt, b"expected-output");
    Ok(())
}

#[tokio::test]
async fn delete_script_converges_after_already_gone_and_repeatable_failures() -> TestResult {
    // Given: deterministic DELETE outcomes and a task-owned workspace.
    let server = MockVidenoa::start().await?;
    let client = MockClient::new(server.base_url())?;
    server.store_file("cleanup/output.mp4", b"output").await?;
    server
        .set_fault(Fault::DeleteScript(vec![
            DeleteOutcome::ServerError,
            DeleteOutcome::ServerError,
            DeleteOutcome::Success,
            DeleteOutcome::NotFound,
        ]))
        .await;

    // When: cleanup retries through two 5xx responses and eventual success.
    assert_eq!(
        client.delete("cleanup").await?,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(
        client.delete("cleanup").await?,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(client.delete("cleanup").await?, StatusCode::NO_CONTENT);
    assert_eq!(client.delete("cleanup").await?, StatusCode::NOT_FOUND);

    // Then: exact attempts are counted and fault evidence can be emitted.
    assert_eq!(server.counters().await.get(Route::DeleteFile), 4);
    server.write_fault_evidence_if_requested().await?;
    Ok(())
}

#[tokio::test]
async fn task_eight_response_and_stall_extensions_remain_wire_level_faults() -> TestResult {
    // Given: the shared harness with a scripted JSON response and stored download.
    let server = MockVidenoa::start().await?;
    let client = MockClient::new(server.base_url())?;
    server
        .set_fault(Fault::Response(ResponseFault {
            route: Route::Health,
            status: 429,
            body: br#"{"error":"limited"}"#.to_vec(),
        }))
        .await;

    // When/Then: the response is a real 429 and the body stall reaches the TCP client.
    assert_eq!(
        client.health_raw().await?.status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    server.store_file("fault/stall.bin", b"bytes").await?;
    server.set_fault(Fault::StallDownload).await;
    assert!(tokio::time::timeout(
        std::time::Duration::from_millis(50),
        client.download("fault/stall.bin")
    )
    .await
    .is_err());
    Ok(())
}
