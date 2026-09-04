use std::error::Error;

use reqwest::StatusCode;
use serde_json::json;

use crate::mock_videnoa::api::MockClient;
use crate::mock_videnoa::domain::JobStatus;
use crate::mock_videnoa::faults::{RestartMode, RestartOutcome};
use crate::mock_videnoa::server::MockVidenoa;

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn persistent_restart_retains_state_and_cancels_active_jobs() -> TestResult {
    // Given: durable files, keyed jobs, and explicit queued/running states.
    let mut server = MockVidenoa::start_persistent().await?;
    let client = MockClient::new(server.base_url())?;
    client
        .upload("restart/input.mkv", b"persistent-input")
        .await?;
    let queued = client
        .run("eligible-workflow.json", "queued-key", json!({"value": 1}))
        .await?;
    let running = client
        .run("eligible-workflow.json", "running-key", json!({"value": 2}))
        .await?;
    server
        .set_job(&running.body.id, JobStatus::Running, None)
        .await?;

    // When: the real listener stops, durable state reloads, and the same address rebinds.
    let outcome = server.restart(RestartMode::RetainState).await?;

    // Then: files/mappings survive and active jobs match Videnoa's cancelled-on-restart rule.
    assert_eq!(outcome, RestartOutcome::Retained);
    assert_eq!(
        client.job(&queued.body.id).await?.status,
        JobStatus::Cancelled
    );
    assert_eq!(
        client.job(&running.body.id).await?.status,
        JobStatus::Cancelled
    );
    assert_eq!(
        client.download("restart/input.mkv").await?,
        b"persistent-input"
    );
    let replay = client
        .run("eligible-workflow.json", "running-key", json!({"value": 2}))
        .await?;
    assert_eq!(replay.status, StatusCode::OK);
    assert_eq!(replay.body.id, running.body.id);
    assert_eq!(server.job_count().await, 2);
    Ok(())
}

#[tokio::test]
async fn state_loss_restart_exposes_ambiguity_without_safe_resubmission_claim() -> TestResult {
    // Given: a durable keyed mapping that exists before restart.
    let mut server = MockVidenoa::start_persistent().await?;
    let client = MockClient::new(server.base_url())?;
    let created = client
        .run("eligible-workflow.json", "lost-key", json!({"value": 1}))
        .await?;

    // When: restart deliberately discards the prior durable state.
    let outcome = server.restart(RestartMode::LoseState).await?;

    // Then: prior evidence is absent and the typed outcome is explicitly ambiguous.
    assert_eq!(outcome, RestartOutcome::StateLostAmbiguous);
    assert_eq!(
        client.job_raw(&created.body.id).await?.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(server.job_count().await, 0);
    assert!(outcome.requires_manual_reconciliation());
    Ok(())
}
