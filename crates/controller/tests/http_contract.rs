use std::error::Error;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use videnoa_controller::{FrontendAssets, app_router};

#[cfg(debug_assertions)]
use axum::http::header;
#[cfg(debug_assertions)]
use std::fs;
#[cfg(debug_assertions)]
use tempfile::TempDir;
#[cfg(debug_assertions)]
use videnoa_controller::StartupError;

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

#[cfg(debug_assertions)]
fn debug_assets() -> Result<(TempDir, FrontendAssets), Box<dyn Error + Send + Sync>> {
    let directory = TempDir::new()?;
    fs::write(
        directory.path().join("index.html"),
        "<!doctype html><title>Controller fixture</title><main>fixture shell</main>",
    )?;
    fs::write(directory.path().join("app.js"), "console.info('fixture')")?;
    let assets = FrontendAssets::from_dist(directory.path())?;
    Ok((directory, assets))
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn health_returns_ok_json_when_controller_is_running() -> TestResult {
    // Given: a Controller router backed by a valid debug asset directory.
    let (_directory, assets) = debug_assets()?;

    // When: the public health endpoint is requested.
    let response = app_router(&assets)
        .oneshot(Request::get("/api/health").body(Body::empty())?)
        .await?;

    // Then: it returns the stable health contract.
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE),
        Some(&header::HeaderValue::from_static("application/json")),
    );
    let body = to_bytes(response.into_body(), 1024).await?;
    assert_eq!(body.as_ref(), br#"{"status":"ok"}"#);
    Ok(())
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn spa_fallback_returns_index_when_route_is_nested() -> TestResult {
    // Given: a Controller router backed by a valid debug SPA fixture.
    let (_directory, assets) = debug_assets()?;

    // When: a client requests a nested client-side route.
    let response = app_router(&assets)
        .oneshot(Request::get("/tasks/example").body(Body::empty())?)
        .await?;

    // Then: the SPA index is returned instead of a misleading 404.
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE),
        Some(&header::HeaderValue::from_static("text/html")),
    );
    let body = to_bytes(response.into_body(), 4096).await?;
    assert!(body.starts_with(b"<!doctype html>"));
    assert!(
        body.windows(b"fixture shell".len())
            .any(|part| part == b"fixture shell")
    );
    Ok(())
}

#[cfg(debug_assertions)]
#[test]
fn debug_startup_returns_typed_error_when_asset_directory_is_missing() -> TestResult {
    // Given: a path whose debug asset directory does not exist.
    let parent = TempDir::new()?;
    let missing = parent.path().join("missing-dist");

    // When: debug assets are opened.
    let error = FrontendAssets::from_dist(&missing)
        .err()
        .ok_or_else(|| std::io::Error::other("missing asset directory unexpectedly succeeded"))?;

    // Then: startup fails with the typed directory error.
    assert!(matches!(
        error,
        StartupError::FrontendDirectoryMissing { .. }
    ));
    Ok(())
}

#[cfg(debug_assertions)]
#[test]
fn debug_startup_returns_typed_error_when_asset_index_becomes_stale() -> TestResult {
    // Given: a formerly valid asset directory whose index has been removed.
    let directory = TempDir::new()?;
    let index = directory.path().join("index.html");
    fs::write(&index, "valid before replacement")?;
    fs::remove_file(index)?;

    // When: debug assets are opened after the stale replacement.
    let error = FrontendAssets::from_dist(directory.path())
        .err()
        .ok_or_else(|| std::io::Error::other("stale asset directory unexpectedly succeeded"))?;

    // Then: startup fails with the typed missing-index error.
    assert!(matches!(error, StartupError::FrontendIndexMissing { .. }));
    Ok(())
}

#[cfg(not(debug_assertions))]
#[tokio::test]
async fn release_binary_serves_embedded_spa_assets() -> TestResult {
    // Given: release assets embedded into the Controller binary.
    let assets = FrontendAssets::embedded()?;

    // When: a nested SPA route is requested without a disk asset directory.
    let response = app_router(&assets)
        .oneshot(Request::get("/release/nested").body(Body::empty())?)
        .await?;

    // Then: the generated Controller GUI is served from the binary.
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024 * 1024).await?;
    assert!(
        body.windows(b"Videnoa Controller".len())
            .any(|part| part == b"Videnoa Controller")
    );
    Ok(())
}
