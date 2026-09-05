use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;
use tokio::sync::Barrier;
use tower::ServiceExt;

use super::support::{fixture_with_busy_timeout, json_body, task_request, TestResult};

#[tokio::test]
async fn concurrent_duplicate_intake_creates_exactly_one_task() -> TestResult {
    let fixture = fixture_with_busy_timeout(Duration::from_millis(100)).await?;
    let body = task_request(&fixture.input, &fixture.output, 1);
    let mut submissions = tokio::task::JoinSet::new();
    let barrier = Arc::new(Barrier::new(9));
    for _ in 0..8 {
        let router = fixture.router.clone();
        let body = body.clone();
        let session = fixture.session.clone();
        let barrier = Arc::clone(&barrier);
        submissions.spawn(async move {
            barrier.wait().await;
            let mut request = session.request("POST", "/api/tasks", Some(&body))?;
            request
                .headers_mut()
                .insert("idempotency-key", "concurrent-key".parse()?);
            let response = router.oneshot(request).await?;
            let status = response.status();
            Ok::<_, Box<dyn Error + Send + Sync>>((status, json_body(response).await?))
        });
    }
    barrier.wait().await;

    let mut created = 0;
    let mut replayed = 0;
    while let Some(result) = submissions.join_next().await {
        match result?? {
            (StatusCode::CREATED, _) => created += 1,
            (StatusCode::OK, _) => replayed += 1,
            (status, body) => {
                return Err(format!("unexpected intake status: {status}; body: {body}").into());
            }
        }
    }
    assert_eq!(created, 1);
    assert_eq!(replayed, 7);

    let response = fixture
        .router
        .oneshot(fixture.session.request("GET", "/api/tasks", None)?)
        .await?;
    assert_eq!(json_body(response).await?["total"], 1);
    Ok(())
}
