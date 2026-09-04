use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use super::support::{fixture, TestResult};

#[tokio::test]
async fn task_routes_reject_anonymous_requests_before_api_fallback() -> TestResult {
    let fixture = fixture().await?;
    for (method, uri) in [
        ("POST", "/api/tasks"),
        ("GET", "/api/tasks"),
        ("GET", "/api/tasks/00000000-0000-4000-8000-000000000001"),
    ] {
        let response = fixture
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    Ok(())
}
