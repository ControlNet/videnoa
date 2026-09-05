use std::time::Duration;

use axum::body::Body;
use axum::http::{header, StatusCode};
use chrono::Duration as ChronoDuration;
use tower::ServiceExt;
use videnoa_controller::auth::{AuthError, SESSION_COOKIE};
use videnoa_controller::config::ControllerConfig;

#[path = "auth_bootstrap/support.rs"]
mod support;
use support::{request, setup, Fixture, TestResult};

#[tokio::test]
async fn existing_session_obeys_tightened_absolute_lifetime() -> TestResult {
    // Given: a session issued under the original long absolute lifetime.
    let fixture = Fixture::new().await?;
    assert!(fixture.database_path.is_file());
    let (cookie, _) = setup(&fixture).await?;
    let token = cookie
        .strip_prefix(&format!("{SESSION_COOKIE}="))
        .ok_or_else(|| std::io::Error::other("unexpected cookie name"))?;
    let issued = fixture
        .auth
        .validate_session_at(token, chrono::Utc::now())
        .await?;
    let mut tightened = ControllerConfig::default().auth;
    tightened.secure_cookie = false;
    tightened.session_absolute = Duration::from_secs(30);
    tightened.session_idle = Duration::from_secs(15);

    // When: policy is tightened and the existing session is checked beyond the new cap.
    fixture.auth.reconfigure(tightened)?;
    let result = fixture
        .auth
        .validate_session_at(token, issued.created_at + ChronoDuration::seconds(31))
        .await;

    // Then: the old stored expiry cannot keep the session authenticated.
    assert!(matches!(result, Err(AuthError::Unauthorized)));
    Ok(())
}

#[tokio::test]
async fn existing_insecure_cookie_session_is_rejected_after_secure_policy_enabled() -> TestResult {
    // Given: a browser session issued while secure cookies were disabled.
    let fixture = Fixture::new().await?;
    let (cookie, _) = setup(&fixture).await?;
    let mut tightened = ControllerConfig::default().auth;
    tightened.secure_cookie = true;

    // When: secure-cookie policy is enabled and the browser reuses its old cookie.
    fixture.auth.reconfigure(tightened)?;
    let mut request = request("GET", "/api/auth/session", Body::empty())?;
    request
        .headers_mut()
        .insert(header::COOKIE, cookie.parse()?);
    let response = fixture.router().oneshot(request).await?;

    // Then: the insecure-policy session is rejected and expired client-side.
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let expired_cookie = response.headers()[header::SET_COOKIE].to_str()?;
    assert!(expired_cookie.starts_with(&format!("{SESSION_COOKIE}=;")));
    assert!(expired_cookie.contains("; Secure"));
    Ok(())
}
