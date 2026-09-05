use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::body::{to_bytes, Body};
use axum::extract::connect_info::ConnectInfo;
use axum::http::{header, Request, StatusCode};
use tower::ServiceExt;

use super::support::{fixture, request_without_peer, TestResult};

#[tokio::test]
async fn direct_router_returns_typed_internal_error_when_peer_metadata_is_missing() -> TestResult {
    // Given: the complete Controller router without listener-provided connection metadata.
    let fixture = fixture().await?;

    for (method, uri) in [
        ("GET", "/api/tasks"),
        ("POST", "/api/tasks"),
        ("GET", "/api/settings"),
        ("PUT", "/api/settings"),
    ] {
        let request = request_without_peer(method, uri, None)?;

        // When: an authenticated task or operations request is routed directly.
        let response = fixture.router.clone().oneshot(request).await?;

        // Then: the wiring defect is represented by the stable typed HTTP boundary.
        assert_eq!(
            response.status(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "{uri}"
        );
        let body = to_bytes(response.into_body(), 4096).await?;
        let payload: serde_json::Value = serde_json::from_slice(&body)?;
        assert_eq!(payload["error"]["code"], "internal_error", "{uri}");
        assert_eq!(payload["error"]["message"], "internal error", "{uri}");
    }
    Ok(())
}

#[tokio::test]
async fn task_routes_reject_anonymous_requests_before_api_fallback() -> TestResult {
    let fixture = fixture().await?;
    for (method, uri) in [
        ("POST", "/api/tasks"),
        ("GET", "/api/tasks"),
        ("GET", "/api/tasks/00000000-0000-4000-8000-000000000001"),
    ] {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())?;
        request.extensions_mut().insert(ConnectInfo(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            40_000,
        )));
        let response = fixture.router.clone().oneshot(request).await?;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    Ok(())
}

#[tokio::test]
async fn protected_task_middleware_returns_typed_rate_limit_response() -> TestResult {
    let fixture = fixture().await?;
    let mut statuses = Vec::new();
    let mut final_body = None;
    for attempt in 0..6 {
        let mut request = Request::builder()
            .uri("/api/tasks")
            .header(header::AUTHORIZATION, "Bearer wrong-secret")
            .body(Body::empty())?;
        request.extensions_mut().insert(ConnectInfo(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            40_000,
        )));
        let response = fixture.router.clone().oneshot(request).await?;
        statuses.push(response.status());
        if attempt == 5 {
            final_body = Some(to_bytes(response.into_body(), 4096).await?);
        }
    }

    assert_eq!(&statuses[..5], &[StatusCode::UNAUTHORIZED; 5]);
    assert_eq!(statuses[5], StatusCode::TOO_MANY_REQUESTS);
    let body = final_body.ok_or_else(|| std::io::Error::other("rate limit body missing"))?;
    let payload: serde_json::Value = serde_json::from_slice(&body)?;
    assert_eq!(payload["error"]["code"], "rate_limited");
    Ok(())
}
