use anyhow::Result;
use axum::http::StatusCode;
use tokio::task::JoinSet;

use super::{persisted_job_count, persisted_job_id, request, response_json, RunFixture};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_duplicate_requests_create_and_dispatch_exactly_one_job() -> Result<()> {
    // Given: twelve callers ready to submit the same key and canonical payload.
    let fixture = RunFixture::new(750)?;
    let router = fixture.router();
    let mut submissions = JoinSet::new();
    for _ in 0..12 {
        let request = request(
            Some("concurrent-key"),
            serde_json::json!({"nested": {"left": 1, "right": 2}}),
        );
        let router = router.clone();
        submissions.spawn(async move { response_json(router, request).await });
    }

    // When: all duplicate requests race through the real router and SQLite file.
    let mut ids = Vec::new();
    let mut created = 0;
    let mut replayed = 0;
    while let Some(result) = submissions.join_next().await {
        let (status, body) = result??;
        match status {
            StatusCode::CREATED => created += 1,
            StatusCode::OK => replayed += 1,
            other => panic!("unexpected duplicate response status: {other}"),
        }
        ids.push(body["id"].as_str().expect("job id").to_string());
    }

    // Then: the database elected one winner before exactly one runtime dispatch.
    assert_eq!(created, 1);
    assert_eq!(replayed, 11);
    assert!(ids.windows(2).all(|pair| pair[0] == pair[1]));
    assert_eq!(persisted_job_count(&fixture.data_dir)?, 1);
    assert_eq!(fixture.state.inner.jobs.len(), 1);
    assert_eq!(fixture.state.inner.progress_senders.len(), 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accept_then_drop_response_replay_returns_persisted_uuid_once() -> Result<()> {
    // Given: a keyed request accepted while execution is deliberately kept observable.
    let fixture = RunFixture::new(750)?;
    let params = serde_json::json!({"input": "lost-response.mkv"});
    let dropped_response = fixture
        .router()
        .oneshot(request(Some("lost-response-key"), params.clone()))
        .await?;
    assert_eq!(dropped_response.status(), StatusCode::CREATED);
    drop(dropped_response);
    let persisted_id = persisted_job_id(&fixture.data_dir)?;

    // When: the client retries without ever reading the accepted response body.
    let (status, replay) =
        response_json(fixture.router(), request(Some("lost-response-key"), params)).await?;

    // Then: replay recovers the durable UUID and no second dispatch occurs.
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay["id"], persisted_id);
    assert_eq!(persisted_job_count(&fixture.data_dir)?, 1);
    assert_eq!(fixture.state.inner.jobs.len(), 1);
    assert_eq!(fixture.state.inner.progress_senders.len(), 1);
    Ok(())
}

use tower::ServiceExt;
