use std::error::Error;

use axum::body::{to_bytes, Body};
use axum::http::{header, Method, Request, StatusCode};
use axum::response::Response;
use tower::ServiceExt;
use videnoa_controller::{app_router, FrontendAssets};

#[cfg(debug_assertions)]
use std::fs;
#[cfg(debug_assertions)]
use tempfile::TempDir;
#[cfg(debug_assertions)]
use videnoa_controller::StartupError;

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

struct TestAssets {
    #[cfg(debug_assertions)]
    _directory: TempDir,
    assets: FrontendAssets,
    spa_marker: &'static [u8],
}

#[cfg(debug_assertions)]
fn test_assets() -> Result<TestAssets, Box<dyn Error + Send + Sync>> {
    let directory = TempDir::new()?;
    fs::write(
        directory.path().join("index.html"),
        "<!doctype html><title>Controller fixture</title><main>fixture shell</main>",
    )?;
    fs::write(directory.path().join("app.js"), "console.info('fixture')")?;
    let assets = FrontendAssets::from_dist(directory.path())?;
    Ok(TestAssets {
        _directory: directory,
        assets,
        spa_marker: b"fixture shell",
    })
}

#[cfg(not(debug_assertions))]
fn test_assets() -> Result<TestAssets, Box<dyn Error + Send + Sync>> {
    Ok(TestAssets {
        assets: FrontendAssets::embedded()?,
        spa_marker: b"Videnoa Controller",
    })
}

async fn assert_api_not_found(method: Method, uri: &'static str) -> TestResult {
    let test_assets = test_assets()?;
    let response = app_router(&test_assets.assets)
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE),
        Some(&header::HeaderValue::from_static("application/json")),
    );
    let body = to_bytes(response.into_body(), 1024).await?;
    assert_eq!(body.as_ref(), br#"{"error":"not_found"}"#);
    Ok(())
}

async fn assert_spa_get_response(response: Response, marker: &'static [u8]) -> TestResult {
    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .ok_or_else(|| std::io::Error::other("SPA response is missing content type"))?;
    assert!(content_type.as_bytes().starts_with(b"text/html"));
    let body = to_bytes(response.into_body(), 1024 * 1024).await?;
    assert!(body.windows(marker.len()).any(|part| part == marker));
    Ok(())
}

async fn assert_spa_head_response(response: Response) -> TestResult {
    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .ok_or_else(|| std::io::Error::other("SPA response is missing content type"))?;
    assert!(content_type.as_bytes().starts_with(b"text/html"));
    let body = to_bytes(response.into_body(), 1024).await?;
    assert!(body.is_empty());
    Ok(())
}

async fn assert_spa_method_rejected(method: Method) -> TestResult {
    let test_assets = test_assets()?;
    let response = app_router(&test_assets.assets)
        .oneshot(
            Request::builder()
                .method(method)
                .uri("/tasks/example")
                .body(Body::empty())?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    let is_html = response
        .headers()
        .get(header::CONTENT_TYPE)
        .is_some_and(|value| value.as_bytes().starts_with(b"text/html"));
    assert!(!is_html);
    let body = to_bytes(response.into_body(), 4096).await?;
    assert!(!body.starts_with(b"<!doctype html>"));
    Ok(())
}

async fn assert_invalid_path_rejected(uri: &'static str) -> TestResult {
    let test_assets = test_assets()?;
    let response = app_router(&test_assets.assets)
        .oneshot(Request::get(uri).body(Body::empty())?)
        .await?;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let is_html = response
        .headers()
        .get(header::CONTENT_TYPE)
        .is_some_and(|value| value.as_bytes().starts_with(b"text/html"));
    assert!(!is_html);
    Ok(())
}

#[tokio::test]
async fn health_returns_ok_json_when_controller_is_running() -> TestResult {
    // Given: a Controller router backed by valid frontend assets.
    let test_assets = test_assets()?;

    // When: the public health endpoint is requested.
    let response = app_router(&test_assets.assets)
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

#[tokio::test]
async fn api_root_returns_not_found_for_get() -> TestResult {
    // Given: the API boundary has no resource at its root.

    // When: a client requests the API root.
    let result = assert_api_not_found(Method::GET, "/api").await;

    // Then: it returns the exact API not-found contract instead of SPA HTML.
    result
}

#[tokio::test]
async fn api_root_with_slash_returns_not_found_for_post() -> TestResult {
    // Given: the API boundary has no resource at its slash-terminated root.

    // When: a client posts to the slash-terminated API root.
    let result = assert_api_not_found(Method::POST, "/api/").await;

    // Then: it returns the exact API not-found contract instead of SPA HTML.
    result
}

#[tokio::test]
async fn unknown_api_route_returns_not_found_for_delete() -> TestResult {
    // Given: the API boundary does not expose the requested resource.

    // When: a client deletes an unknown API route.
    let result = assert_api_not_found(Method::DELETE, "/api/unknown").await;

    // Then: it returns the exact API not-found contract instead of SPA HTML.
    result
}

#[tokio::test]
async fn encoded_api_spellings_return_not_found_for_get() -> TestResult {
    // Given: API paths with encoded names, separators, and percent-hex casing.
    let encoded_paths = [
        "/api%2Funknown",
        "/api%2F",
        "/%61pi/unknown",
        "/api%2funknown",
    ];

    // When: a client requests each raw encoded path.
    for path in encoded_paths {
        assert_api_not_found(Method::GET, path).await?;
    }

    // Then: every decoded API boundary returned the exact JSON error.
    Ok(())
}

#[tokio::test]
async fn invalid_percent_encoding_returns_bad_request() -> TestResult {
    // Given: incomplete percent encoding and percent-encoded invalid UTF-8.
    let invalid_paths = ["/api%2", "/%FFapi/unknown"];

    // When: a client requests each invalid raw path.
    for path in invalid_paths {
        assert_invalid_path_rejected(path).await?;
    }

    // Then: neither path reached the SPA fallback.
    Ok(())
}

#[tokio::test]
async fn spa_fallback_rejects_post_when_route_is_nested() -> TestResult {
    // Given: a nested client-side route.

    // When: a client posts to a nested client-side route.
    let result = assert_spa_method_rejected(Method::POST).await;

    // Then: both profiles reject the method without serving SPA HTML.
    result
}

#[tokio::test]
async fn spa_fallback_rejects_options_when_route_is_nested() -> TestResult {
    // Given: a nested client-side route.

    // When: a client sends OPTIONS to a nested client-side route.
    let result = assert_spa_method_rejected(Method::OPTIONS).await;

    // Then: both profiles reject the method without serving SPA HTML.
    result
}

#[tokio::test]
async fn spa_fallback_returns_index_when_route_is_root() -> TestResult {
    // Given: a Controller router backed by valid frontend assets.
    let test_assets = test_assets()?;

    // When: a client requests the root client-side route.
    let response = app_router(&test_assets.assets)
        .oneshot(Request::get("/").body(Body::empty())?)
        .await?;

    // Then: the actual SPA index body is returned.
    assert_spa_get_response(response, test_assets.spa_marker).await
}

#[tokio::test]
async fn spa_fallback_returns_index_when_route_is_nested() -> TestResult {
    // Given: a Controller router backed by valid frontend assets.
    let test_assets = test_assets()?;

    // When: a client requests a nested client-side route.
    let response = app_router(&test_assets.assets)
        .oneshot(Request::get("/tasks/example").body(Body::empty())?)
        .await?;

    // Then: the actual SPA index body is returned instead of a misleading 404.
    assert_spa_get_response(response, test_assets.spa_marker).await
}

#[tokio::test]
async fn spa_fallback_returns_head_for_client_routes() -> TestResult {
    // Given: a Controller router backed by valid frontend assets.
    let test_assets = test_assets()?;

    // When: a client sends HEAD to root and nested client-side routes.
    for path in ["/", "/tasks/example"] {
        let response = app_router(&test_assets.assets)
            .oneshot(
                Request::builder()
                    .method(Method::HEAD)
                    .uri(path)
                    .body(Body::empty())?,
            )
            .await?;
        assert_spa_head_response(response).await?;
    }

    // Then: both responses contain SPA metadata with empty bodies.
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
    assert!(body
        .windows(b"Videnoa Controller".len())
        .any(|part| part == b"Videnoa Controller"));
    Ok(())
}
