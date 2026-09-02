use anyhow::Result;
use axum::http::StatusCode;

use super::{persisted_job_count, request, response_json, JobStatus, RunFixture};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn replay_after_restart_returns_same_cancelled_job_without_replacement() -> Result<()> {
    // Given: a keyed job durably queued while the old executor is blocked.
    let fixture = RunFixture::new(500)?;
    let permit = fixture
        .state
        .inner
        .gpu_semaphore
        .clone()
        .acquire_owned()
        .await?;
    let params = serde_json::json!({"input": "restart.mkv"});
    let (created_status, created) = response_json(
        fixture.router(),
        request(Some("restart-key"), params.clone()),
    )
    .await?;
    assert_eq!(created_status, StatusCode::CREATED);
    let job_id = created["id"].as_str().expect("job id").to_string();
    fixture
        .state
        .inner
        .jobs
        .get(&job_id)
        .expect("queued job")
        .cancel_token
        .cancel();
    tokio::task::yield_now().await;

    // When: a new AppState starts from the same database and receives the replay.
    let restarted = fixture.restarted_state();
    let restored = restarted.inner.jobs.get(&job_id).expect("restored job");
    assert_eq!(restored.status, JobStatus::Cancelled);
    drop(restored);
    let (replay_status, replay) = response_json(
        super::app_router(restarted.clone()),
        request(Some("restart-key"), params),
    )
    .await?;

    // Then: the original UUID and restart-cancelled status remain visible.
    assert_eq!(replay_status, StatusCode::OK);
    assert_eq!(replay["id"], job_id);
    assert_eq!(replay["status"], "cancelled");
    assert_eq!(persisted_job_count(&fixture.data_dir)?, 1);
    assert_eq!(restarted.inner.jobs.len(), 1);
    drop(permit);
    Ok(())
}
