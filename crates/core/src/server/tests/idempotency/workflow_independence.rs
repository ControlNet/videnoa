use anyhow::Result;
use axum::http::StatusCode;
use sha2::{Digest, Sha256};

use super::{
    persisted_job_count, request, response_json, write_workflow, RunFixture, WORKFLOW_NAME,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn identical_replay_succeeds_after_workflow_is_removed() -> Result<()> {
    // Given: a keyed job accepted while its workflow file exists.
    let fixture = RunFixture::new(500)?;
    let params = serde_json::json!({"input": "removed.mkv"});
    let (_, created) = response_json(
        fixture.router(),
        request(Some("removed-workflow-key"), params.clone()),
    )
    .await?;
    std::fs::remove_file(fixture.workflows_dir.join(format!("{WORKFLOW_NAME}.json")))?;

    // When: the identical request is replayed after removal.
    let (status, replay) = response_json(
        fixture.router(),
        request(Some("removed-workflow-key"), params),
    )
    .await?;

    // Then: durable state returns the original job without consulting the file.
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay["id"], created["id"]);
    assert_eq!(persisted_job_count(&fixture.data_dir)?, 1);
    assert_eq!(fixture.state.inner.jobs.len(), 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn identical_replay_succeeds_after_workflow_becomes_invalid() -> Result<()> {
    // Given: a keyed job accepted before its workflow file is corrupted.
    let fixture = RunFixture::new(500)?;
    let params = serde_json::json!({"input": "corrupt.mkv"});
    let (_, created) = response_json(
        fixture.router(),
        request(Some("corrupt-workflow-key"), params.clone()),
    )
    .await?;
    std::fs::write(
        fixture.workflows_dir.join(format!("{WORKFLOW_NAME}.json")),
        b"not valid json",
    )?;

    // When: the identical request is replayed after corruption.
    let (status, replay) = response_json(
        fixture.router(),
        request(Some("corrupt-workflow-key"), params),
    )
    .await?;

    // Then: durable state returns the original job without parsing the file.
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay["id"], created["id"]);
    assert_eq!(persisted_job_count(&fixture.data_dir)?, 1);
    assert_eq!(fixture.state.inner.jobs.len(), 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn identical_replay_succeeds_after_workflow_content_changes() -> Result<()> {
    // Given: a keyed job accepted from the original valid workflow.
    let fixture = RunFixture::new(500)?;
    let params = serde_json::json!({"input": "changed.mkv"});
    let (_, created) = response_json(
        fixture.router(),
        request(Some("changed-workflow-key"), params.clone()),
    )
    .await?;
    write_workflow(&fixture.workflows_dir, 0)?;

    // When: the identical request is replayed after valid content changes.
    let (status, replay) = response_json(
        fixture.router(),
        request(Some("changed-workflow-key"), params),
    )
    .await?;

    // Then: workflow content remains outside the locked fingerprint contract.
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay["id"], created["id"]);
    assert_eq!(persisted_job_count(&fixture.data_dir)?, 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn changed_payload_conflicts_after_workflow_is_removed() -> Result<()> {
    // Given: a key bound to one request before its workflow is removed.
    let fixture = RunFixture::new(500)?;
    response_json(
        fixture.router(),
        request(Some("missing-conflict-key"), serde_json::json!({"seed": 1})),
    )
    .await?;
    std::fs::remove_file(fixture.workflows_dir.join(format!("{WORKFLOW_NAME}.json")))?;

    // When: the key is reused with changed params.
    let (status, body) = response_json(
        fixture.router(),
        request(Some("missing-conflict-key"), serde_json::json!({"seed": 2})),
    )
    .await?;

    // Then: the durable key collision wins over current workflow availability.
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "idempotency_conflict");
    assert_eq!(persisted_job_count(&fixture.data_dir)?, 1);
    assert_eq!(fixture.state.inner.jobs.len(), 1);
    Ok(())
}

#[tokio::test]
async fn new_key_with_unavailable_workflow_creates_nothing() -> Result<()> {
    // Given: a fresh key whose named workflow file is unavailable.
    let fixture = RunFixture::new(0)?;
    std::fs::remove_file(fixture.workflows_dir.join(format!("{WORKFLOW_NAME}.json")))?;

    // When: the genuinely new request is submitted.
    let (status, _) = response_json(
        fixture.router(),
        request(Some("new-missing-key"), serde_json::json!({"seed": 1})),
    )
    .await?;

    // Then: validation fails before persistence or runtime dispatch.
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(persisted_job_count(&fixture.data_dir)?, 0);
    assert!(fixture.state.inner.jobs.is_empty());
    assert!(fixture.state.inner.progress_senders.is_empty());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn json_equivalent_numeric_spellings_share_one_fingerprint() -> Result<()> {
    // Given: one keyed request containing an integral JSON number.
    let fixture = RunFixture::new(500)?;
    let (_, created) = response_json(
        fixture.router(),
        request(Some("numeric-key"), serde_json::json!({"value": 1})),
    )
    .await?;

    // When: the same mathematical JSON number is replayed as 1.0.
    let (status, replay) = response_json(
        fixture.router(),
        request(Some("numeric-key"), serde_json::json!({"value": 1.0})),
    )
    .await?;

    // Then: standards-consistent canonicalization returns the original job.
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay["id"], created["id"]);
    assert_eq!(persisted_job_count(&fixture.data_dir)?, 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn numeric_replay_accepts_fingerprint_persisted_before_normalization() -> Result<()> {
    // Given: a keyed float request whose row carries the original Task 5 fingerprint.
    let fixture = RunFixture::new(500)?;
    let (_, created) = response_json(
        fixture.router(),
        request(
            Some("legacy-numeric-key"),
            serde_json::json!({"value": 1.0}),
        ),
    )
    .await?;
    let legacy = format!(
        "{:x}",
        Sha256::digest(br#"{"params":{"value":1.0},"workflow_name":"idempotent-run"}"#)
    );
    rusqlite::Connection::open(fixture.data_dir.join("jobs.db"))?.execute(
        "UPDATE jobs SET request_fingerprint = ?1 WHERE id = ?2",
        rusqlite::params![legacy, created["id"].as_str().expect("job id")],
    )?;

    // When: the equivalent integral spelling is replayed after the upgrade.
    let (status, replay) = response_json(
        fixture.router(),
        request(Some("legacy-numeric-key"), serde_json::json!({"value": 1})),
    )
    .await?;

    // Then: the persisted request snapshot bridges the fingerprint normalization.
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay["id"], created["id"]);
    assert_eq!(persisted_job_count(&fixture.data_dir)?, 1);
    Ok(())
}
