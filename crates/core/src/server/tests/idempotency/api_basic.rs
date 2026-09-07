use anyhow::Result;
use axum::http::StatusCode;

use super::{persisted_job_count, request, response_json, RunFixture};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn omitted_header_preserves_unkeyed_creation_contract() -> Result<()> {
    // Given: an existing client request with no idempotency header.
    let fixture = RunFixture::new(200)?;

    // When: the request is submitted through the existing endpoint.
    let (status, body) = response_json(
        fixture.router(),
        request(None, serde_json::json!({"input": "episode.mkv"})),
    )
    .await?;

    // Then: the legacy 201 response shape and one new row remain unchanged.
    assert_eq!(status, StatusCode::CREATED);
    assert!(body["id"].is_string());
    assert_eq!(body["status"], "queued");
    assert!(body["created_at"].is_string());
    assert_eq!(persisted_job_count(&fixture.data_dir)?, 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn first_keyed_submission_persists_mapping_and_returns_created() -> Result<()> {
    // Given: a fresh durable idempotency key.
    let fixture = RunFixture::new(200)?;

    // When: the keyed workflow is submitted once.
    let (status, body) = response_json(
        fixture.router(),
        request(Some("first-key"), serde_json::json!({"seed": 7})),
    )
    .await?;

    // Then: the job is created and its durable mapping is populated.
    assert_eq!(status, StatusCode::CREATED);
    let connection = rusqlite::Connection::open(fixture.data_dir.join("jobs.db"))?;
    let (has_key, fingerprint_length): (bool, u64) = connection.query_row(
        "SELECT idempotency_key IS NOT NULL, length(request_fingerprint) FROM jobs WHERE id = ?1",
        rusqlite::params![body["id"].as_str().expect("job id")],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    assert!(has_key);
    assert_eq!(fingerprint_length, 64);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn identical_sequential_replay_returns_original_job_without_queueing() -> Result<()> {
    // Given: one accepted keyed submission.
    let fixture = RunFixture::new(500)?;
    let params = serde_json::json!({"input": "episode.mkv", "seed": 42});
    let (first_status, first) = response_json(
        fixture.router(),
        request(Some("replay-key"), params.clone()),
    )
    .await?;

    // When: the same key and payload are submitted again.
    let (replay_status, replay) =
        response_json(fixture.router(), request(Some("replay-key"), params)).await?;

    // Then: the original UUID is returned and only one runtime dispatch exists.
    assert_eq!(first_status, StatusCode::CREATED);
    assert_eq!(replay_status, StatusCode::OK);
    assert_eq!(replay["id"], first["id"]);
    assert_eq!(persisted_job_count(&fixture.data_dir)?, 1);
    assert_eq!(fixture.state.inner.jobs.len(), 1);
    assert_eq!(fixture.state.inner.progress_senders.len(), 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nested_object_order_and_json_formatting_share_one_fingerprint() -> Result<()> {
    // Given: semantically equal nested params with different object order and formatting.
    let fixture = RunFixture::new(300)?;
    let first = br#"{"workflow_name":"idempotent-run","params":{"outer":{"b":2,"a":{"y":true,"x":null}},"items":[3,2,1]}}"#;
    let reordered = br#"{
        "params": {"items": [3, 2, 1], "outer": {"a": {"x": null, "y": true}, "b": 2}},
        "workflow_name": "idempotent-run"
    }"#;
    let (first_status, first_body) = super::response_json(
        fixture.router(),
        super::raw_request(Some("canonical-key"), first.to_vec()),
    )
    .await?;

    // When: the reordered representation is replayed.
    let (replay_status, replay_body) = super::response_json(
        fixture.router(),
        super::raw_request(Some("canonical-key"), reordered.to_vec()),
    )
    .await?;

    // Then: canonicalization treats them as one submission and preserves array order.
    assert_eq!(first_status, StatusCode::CREATED);
    assert_eq!(replay_status, StatusCode::OK);
    assert_eq!(replay_body["id"], first_body["id"]);
    assert_eq!(persisted_job_count(&fixture.data_dir)?, 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn changed_payload_reuse_returns_stable_conflict_without_queueing() -> Result<()> {
    // Given: a key already bound to one payload.
    let fixture = RunFixture::new(500)?;
    let (_, first) = response_json(
        fixture.router(),
        request(Some("collision-key"), serde_json::json!({"seed": 1})),
    )
    .await?;

    // When: the key is reused with changed params.
    let (status, body) = response_json(
        fixture.router(),
        request(Some("collision-key"), serde_json::json!({"seed": 2})),
    )
    .await?;

    // Then: a stable typed conflict is returned without request internals or a second job.
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "idempotency_conflict");
    assert_eq!(
        body["error"],
        "idempotency key conflicts with an existing submission"
    );
    assert!(!body.to_string().contains("collision-key"));
    assert!(!body.to_string().contains("seed"));
    assert_eq!(persisted_job_count(&fixture.data_dir)?, 1);
    assert_eq!(fixture.state.inner.jobs.len(), 1);
    assert_eq!(
        fixture.state.inner.jobs.iter().next().expect("job").id,
        first["id"]
    );
    Ok(())
}

#[tokio::test]
async fn malformed_empty_and_oversized_keys_return_stable_client_error() -> Result<()> {
    // Given: malformed key values at each validation boundary.
    let fixture = RunFixture::new(0)?;
    let oversized = "a".repeat(256);

    // When: each invalid key is submitted.
    for key in ["", "contains space", oversized.as_str()] {
        let (status, body) = response_json(
            fixture.router(),
            request(Some(key), serde_json::json!({"seed": 1})),
        )
        .await?;

        // Then: the same typed error is returned and no job is created.
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["code"], "invalid_idempotency_key");
        assert_eq!(body["error"], "invalid idempotency key");
    }
    assert_eq!(persisted_job_count(&fixture.data_dir)?, 0);
    Ok(())
}
