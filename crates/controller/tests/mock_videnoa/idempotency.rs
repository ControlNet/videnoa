use std::collections::BTreeSet;
use std::error::Error;

use reqwest::StatusCode;
use serde_json::json;
use tokio::task::JoinSet;

use crate::mock_videnoa::client::MockClient;
use crate::mock_videnoa::journal::Route;
use crate::mock_videnoa::server::MockVidenoa;

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

#[tokio::test]
async fn sequential_same_key_replays_one_remote_job() -> TestResult {
    // Given: one deterministic keyed request.
    let server = MockVidenoa::start().await?;
    let client = MockClient::new(server.base_url())?;
    let params = json!({"nested": {"left": 1, "right": 2}});

    // When: the same key and canonical body are submitted twice.
    let first = client
        .run("eligible-workflow.json", "same-key", params.clone())
        .await?;
    let replay = client
        .run("eligible-workflow.json", "same-key", params)
        .await?;

    // Then: creation occurs once and replay returns the same durable job.
    assert_eq!(first.status, StatusCode::CREATED);
    assert_eq!(replay.status, StatusCode::OK);
    assert_eq!(first.body.id, replay.body.id);
    assert_eq!(server.job_count().await, 1);
    assert_eq!(server.counters().await.get(Route::Run), 2);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_same_key_elects_exactly_one_creator() -> TestResult {
    // Given: twelve callers racing with one key and equivalent payload.
    let server = MockVidenoa::start().await?;
    let client = MockClient::new(server.base_url())?;
    let mut submissions = JoinSet::new();
    for _ in 0..12 {
        let client = client.clone();
        submissions.spawn(async move {
            client
                .run(
                    "eligible-workflow.json",
                    "concurrent-key",
                    json!({"number": 1.0, "nested": {"right": 2, "left": 1}}),
                )
                .await
        });
    }

    // When: every response is collected.
    let mut created = 0;
    let mut replayed = 0;
    let mut ids = BTreeSet::new();
    while let Some(result) = submissions.join_next().await {
        let response = result??;
        match response.status {
            StatusCode::CREATED => created += 1,
            StatusCode::OK => replayed += 1,
            status => return Err(std::io::Error::other(format!("unexpected {status}")).into()),
        }
        ids.insert(response.body.id);
    }

    // Then: the authoritative state contains one job and one UUID.
    assert_eq!(created, 1);
    assert_eq!(replayed, 11);
    assert_eq!(ids.len(), 1);
    assert_eq!(server.job_count().await, 1);
    Ok(())
}

#[tokio::test]
async fn changed_payload_for_same_key_returns_conflict() -> TestResult {
    // Given: an existing key mapped to one canonical body.
    let server = MockVidenoa::start().await?;
    let client = MockClient::new(server.base_url())?;
    client
        .run(
            "eligible-workflow.json",
            "collision-key",
            json!({"value": 1}),
        )
        .await?;

    // When: the key is reused with changed content.
    let response = client
        .run_raw(
            "eligible-workflow.json",
            "collision-key",
            json!({"value": 2}),
        )
        .await?;

    // Then: the mock matches Videnoa's durable 409 contract without dispatching again.
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(server.job_count().await, 1);
    Ok(())
}
