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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lookup_waits_for_in_flight_idempotent_claim_and_returns_accepted_job() -> Result<()> {
    let fixture = RunFixture::new(0)?;
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel::<String>();
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let entered_tx = std::sync::Mutex::new(Some(entered_tx));
    let release_rx = std::sync::Mutex::new(release_rx);
    *fixture
        .state
        .inner
        .after_job_claim_hook
        .lock()
        .expect("claim hook lock") = Some(std::sync::Arc::new(move |job_id: &str| {
        if let Some(tx) = entered_tx.lock().expect("entered lock").take() {
            let _ = tx.send(job_id.to_owned());
        }
        let _ = release_rx.lock().expect("release lock").recv();
    }));

    let key = "lookup-in-flight-commit-key";
    let submit_router = fixture.router();
    let submit_task = tokio::spawn(async move {
        response_json(
            submit_router,
            request(Some(key), serde_json::json!({"input": "a.mkv"})),
        )
        .await
    });

    let claimed_job_id = tokio::time::timeout(std::time::Duration::from_secs(5), entered_rx)
        .await
        .expect("POST /api/run reached claim hook")?;

    // Start GET /api/jobs/by-idempotency-key/{key} while POST /api/run is still
    // in `admitting` before runtime publication. It must NOT return a premature 404.
    let lookup_router = fixture.router();
    let mut lookup_task =
        tokio::spawn(async move { response_json(lookup_router, lookup(key)).await });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(
        !lookup_task.is_finished(),
        "lookup must serialize behind the in-flight idempotent submission instead of returning premature 404"
    );

    release_tx.send(()).expect("release claim hook");

    let (submit_status, submit_body) = submit_task.await??;
    assert_eq!(submit_status, StatusCode::CREATED, "{submit_body}");
    assert_eq!(submit_body["id"], claimed_job_id);

    let (lookup_status, lookup_body) =
        tokio::time::timeout(std::time::Duration::from_secs(5), &mut lookup_task)
            .await
            .expect("lookup completed after commit")??;
    assert_eq!(lookup_status, StatusCode::OK, "{lookup_body}");
    assert_eq!(lookup_body["id"], claimed_job_id);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lookup_returns_not_found_after_in_flight_claim_rolls_back_on_shutdown() -> Result<()> {
    let fixture = RunFixture::new(0)?;
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel::<String>();
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let entered_tx = std::sync::Mutex::new(Some(entered_tx));
    let release_rx = std::sync::Mutex::new(release_rx);
    *fixture
        .state
        .inner
        .after_job_claim_hook
        .lock()
        .expect("claim hook lock") = Some(std::sync::Arc::new(move |job_id: &str| {
        if let Some(tx) = entered_tx.lock().expect("entered lock").take() {
            let _ = tx.send(job_id.to_owned());
        }
        let _ = release_rx.lock().expect("release lock").recv();
    }));

    let key = "lookup-in-flight-rollback-key";
    let submit_router = fixture.router();
    let submit_task = tokio::spawn(async move {
        response_json(
            submit_router,
            request(Some(key), serde_json::json!({"input": "a.mkv"})),
        )
        .await
    });

    let _claimed_job_id = tokio::time::timeout(std::time::Duration::from_secs(5), entered_rx)
        .await
        .expect("POST /api/run reached claim hook")?;

    let lookup_router = fixture.router();
    let lookup_task = tokio::spawn(async move { response_json(lookup_router, lookup(key)).await });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(
        !lookup_task.is_finished(),
        "lookup must wait for in-flight submission outcome"
    );

    // Trigger shutdown so the in-flight submission rolls back instead of accepting.
    fixture.state.begin_shutdown();
    release_tx.send(()).expect("release claim hook");

    let (submit_status, submit_body) = submit_task.await??;
    assert_eq!(
        submit_status,
        StatusCode::SERVICE_UNAVAILABLE,
        "{submit_body}"
    );

    let (lookup_status, lookup_body) = lookup_task.await??;
    assert_eq!(lookup_status, StatusCode::NOT_FOUND, "{lookup_body}");
    assert_eq!(lookup_body["code"], "job_not_found", "{lookup_body}");
    assert_eq!(persisted_job_count(&fixture.data_dir)?, 0);
    Ok(())
}
