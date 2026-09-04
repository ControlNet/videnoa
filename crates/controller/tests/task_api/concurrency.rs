use std::error::Error;

use axum::http::StatusCode;
use tower::ServiceExt;

use super::support::{fixture, json_body, request, task_request, TestResult};

#[tokio::test]
async fn concurrent_duplicate_intake_creates_exactly_one_task() -> TestResult {
    let fixture = fixture().await?;
    let body = task_request(&fixture.input, &fixture.output, 1);
    let mut submissions = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let router = fixture.router.clone();
        let body = body.clone();
        submissions.spawn(async move {
            let mut request = request("POST", "/api/tasks", Some(&body))?;
            request
                .headers_mut()
                .insert("idempotency-key", "concurrent-key".parse()?);
            let response = router.oneshot(request).await?;
            Ok::<_, Box<dyn Error + Send + Sync>>(response.status())
        });
    }

    let mut created = 0;
    let mut replayed = 0;
    while let Some(result) = submissions.join_next().await {
        match result?? {
            StatusCode::CREATED => created += 1,
            StatusCode::OK => replayed += 1,
            status => return Err(format!("unexpected intake status: {status}").into()),
        }
    }
    assert_eq!(created, 1);
    assert_eq!(replayed, 7);

    let response = fixture
        .router
        .oneshot(request("GET", "/api/tasks", None)?)
        .await?;
    assert_eq!(json_body(response).await?["total"], 1);
    Ok(())
}
