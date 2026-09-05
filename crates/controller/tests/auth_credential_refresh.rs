use std::net::{IpAddr, Ipv4Addr};

use axum::body::Body;
use axum::http::{header, StatusCode};
use chrono::Utc;
use tower::ServiceExt;
use videnoa_controller::auth::{hash_password, AuthError};
use videnoa_controller::domain::SecretString;
use videnoa_controller::persistence::{Database, DatabaseOptions};

#[path = "auth_bootstrap/support.rs"]
mod support;
use support::{request, setup, Fixture, TestResult, PASSWORD};

#[tokio::test]
async fn readiness_observes_credential_deletion_after_cache_warmup() -> TestResult {
    // Given: setup has populated the service's parsed credential cache.
    let fixture = Fixture::new().await?;
    let _session = setup(&fixture).await?;
    assert!(fixture.auth.initialized().await?);
    let database = Database::open(DatabaseOptions::new(&fixture.database_path)).await?;

    // When: the durable singleton credential is removed outside the auth service.
    sqlx::query("DELETE FROM administrator_credential WHERE id = 1")
        .execute(database.pool())
        .await?;

    // Then: readiness reflects current SQLite state rather than stale cached state.
    assert!(!fixture.auth.initialized().await?);
    assert!(matches!(
        fixture.auth.check_ready().await,
        Err(AuthError::Unauthorized)
    ));
    Ok(())
}

#[tokio::test]
async fn sessions_and_login_observe_credential_rotation_after_cache_warmup() -> TestResult {
    // Given: setup issued a session and cached the original parsed credential.
    let fixture = Fixture::new().await?;
    let (cookie, _) = setup(&fixture).await?;
    assert!(fixture.auth.initialized().await?);
    let replacement_credential = "test-only-rotated-password";
    let replacement_hash = hash_password(replacement_credential)?;
    let database = Database::open(DatabaseOptions::new(&fixture.database_path)).await?;

    // When: the durable credential is rotated outside the auth service.
    sqlx::query("UPDATE administrator_credential SET password_hash = ? WHERE id = 1")
        .bind(replacement_hash)
        .execute(database.pool())
        .await?;

    // Then: the old session and password fail while the replacement password succeeds.
    let mut session = request("GET", "/api/auth/session", Body::empty())?;
    session
        .headers_mut()
        .insert(header::COOKIE, cookie.parse()?);
    let session = fixture.router().oneshot(session).await?;
    assert_eq!(session.status(), StatusCode::UNAUTHORIZED);
    assert!(matches!(
        fixture
            .auth
            .login(
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                &SecretString::new(PASSWORD),
                Utc::now(),
            )
            .await,
        Err(AuthError::Unauthorized)
    ));
    assert!(fixture
        .auth
        .login(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            &SecretString::new(replacement_credential),
            Utc::now(),
        )
        .await
        .is_ok());
    Ok(())
}
