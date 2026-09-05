use std::fs;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use serde_json::json;
use tower::ServiceExt;

use super::support::{connected_request, json_body, Fixture, TestResult, PASSWORD};

#[tokio::test]
async fn operational_routes_require_authentication() -> TestResult {
    let fixture = Fixture::new().await?;
    for uri in [
        "/api/workers",
        "/api/settings",
        "/api/status-counts",
        "/api/events",
    ] {
        let request = connected_request(Request::builder().uri(uri).body(Body::empty())?, 40_000);
        let response = fixture.router.clone().oneshot(request).await?;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{uri}");
    }
    Ok(())
}

#[tokio::test]
async fn cookie_mutations_require_same_origin_csrf_proof() -> TestResult {
    let fixture = Fixture::new().await?;
    let login = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(
            &json!({"password": PASSWORD}),
        )?))?;
    let response = fixture
        .router
        .clone()
        .oneshot(connected_request(login, 40_123))
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .ok_or("session cookie missing")?
        .to_str()?
        .split(';')
        .next()
        .ok_or("session cookie value missing")?
        .to_owned();
    let csrf = response
        .headers()
        .get("x-csrf-token")
        .ok_or("csrf proof missing")?
        .to_str()?
        .to_owned();
    let worker = json!({
        "name": "session-worker",
        "api_url": "https://session-worker.example/api/",
        "enabled": true,
        "compute_slots": 1
    });

    let forbidden = Request::builder()
        .method("POST")
        .uri("/api/workers")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::COOKIE, &cookie)
        .body(Body::from(serde_json::to_vec(&worker)?))?;
    let forbidden = fixture
        .router
        .clone()
        .oneshot(connected_request(forbidden, 40_000))
        .await?;
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

    let accepted = Request::builder()
        .method("POST")
        .uri("/api/workers")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::COOKIE, cookie)
        .header(header::HOST, "controller.test")
        .header(header::ORIGIN, "http://controller.test")
        .header("x-csrf-token", csrf)
        .body(Body::from(serde_json::to_vec(&worker)?))?;
    let accepted = fixture
        .router
        .clone()
        .oneshot(connected_request(accepted, 40_000))
        .await?;
    assert_eq!(accepted.status(), StatusCode::CREATED);
    Ok(())
}

#[tokio::test]
async fn readiness_fails_when_a_retained_root_is_replaced() -> TestResult {
    let fixture = Fixture::new().await?;
    let moved = fixture.workspace.with_extension("replaced");
    fs::rename(&fixture.workspace, &moved)?;
    fs::create_dir(&fixture.workspace)?;

    let response = fixture
        .router
        .clone()
        .oneshot(Fixture::request("GET", "/api/readiness", None)?)
        .await?;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let readiness = json_body(response).await?;
    assert_eq!(readiness["status"], "not_ready");
    assert_eq!(readiness["checks"][2]["name"], "root_handles");
    assert_eq!(readiness["checks"][2]["ready"], false);
    Ok(())
}

#[tokio::test]
async fn readiness_reports_invalid_authentication_material() -> TestResult {
    let fixture = Fixture::new().await?;
    let mut connection = fixture.store.database().pool().acquire().await?;
    sqlx::query("PRAGMA ignore_check_constraints = ON")
        .execute(&mut *connection)
        .await?;
    sqlx::query(
        "UPDATE administrator_credential SET password_hash = 'invalid-password-hash' WHERE id = 1",
    )
    .execute(&mut *connection)
    .await?;

    let response = fixture
        .router
        .clone()
        .oneshot(Fixture::request("GET", "/api/readiness", None)?)
        .await?;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let readiness = json_body(response).await?;
    assert_eq!(readiness["status"], "not_ready");
    assert_eq!(readiness["checks"][1]["name"], "authentication");
    assert_eq!(readiness["checks"][1]["ready"], false);
    Ok(())
}
