use anyhow::Result;
use axum::body::Body;
use axum::http::{Request, StatusCode};

use super::{persisted_job_count, request, response_json, RunFixture};

fn lookup(key: &str) -> Request<Body> {
    Request::builder()
        .uri(format!("/api/jobs/by-idempotency-key/{key}"))
        .body(Body::empty())
        .expect("build request")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_submission_is_found_by_its_key_without_creating_a_job() -> Result<()> {
    // Given: a worker that has accepted no run with the key yet.
    let fixture = RunFixture::new(0)?;
    let (status, body) = response_json(fixture.router(), lookup("lookup-key")).await?;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["code"], "job_not_found", "{body}");
    assert_eq!(persisted_job_count(&fixture.data_dir)?, 0);

    // When: a run with the key is accepted and then looked up.
    let (created_status, created) = response_json(
        fixture.router(),
        request(Some("lookup-key"), serde_json::json!({"input": "a.mkv"})),
    )
    .await?;
    assert_eq!(created_status, StatusCode::CREATED);
    let (status, found) = response_json(fixture.router(), lookup("lookup-key")).await?;

    // Then: the lookup names the job the run created and creates none itself.
    assert_eq!(status, StatusCode::OK, "{found}");
    assert_eq!(found["id"], created["id"]);
    assert_eq!(found["created_at"], created["created_at"]);
    assert!(found["status"].is_string());
    assert_eq!(persisted_job_count(&fixture.data_dir)?, 1);
    assert_eq!(fixture.state.inner.jobs.len(), 1);

    // And: a key /api/run would refuse is refused here too.
    let (status, body) = response_json(fixture.router(), lookup("bad%20key")).await?;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "invalid_idempotency_key", "{body}");
    Ok(())
}
