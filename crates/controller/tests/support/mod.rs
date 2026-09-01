use std::error::Error;

use axum::body::{to_bytes, Body, Bytes};
use axum::http::{header, HeaderValue, Method, Request, StatusCode};
use axum::response::Response;
use tower::ServiceExt;
use videnoa_controller::{app_router, FrontendAssets};

#[cfg(debug_assertions)]
use std::fs;
#[cfg(debug_assertions)]
use tempfile::TempDir;

pub type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

pub struct TestAssets {
    #[cfg(debug_assertions)]
    _directory: TempDir,
    pub assets: FrontendAssets,
    pub spa_marker: &'static [u8],
}

pub struct AssetResponse {
    pub status: StatusCode,
    pub content_type: HeaderValue,
    pub body: Bytes,
}

#[cfg(debug_assertions)]
pub fn test_assets() -> Result<TestAssets, Box<dyn Error + Send + Sync>> {
    let directory = TempDir::new()?;
    fs::write(
        directory.path().join("index.html"),
        "<!doctype html><title>Controller fixture</title><main>fixture shell</main>",
    )?;
    fs::write(directory.path().join("app.js"), "console.info('fixture')")?;
    fs::create_dir(directory.path().join("assets"))?;
    fs::write(
        directory.path().join("assets/controller-shell.js"),
        "console.info('encoded fixture asset')",
    )?;
    let assets = FrontendAssets::from_dist(directory.path())?;
    Ok(TestAssets {
        _directory: directory,
        assets,
        spa_marker: b"fixture shell",
    })
}

#[cfg(debug_assertions)]
pub async fn static_asset_path(_: &TestAssets) -> Result<String, Box<dyn Error + Send + Sync>> {
    Ok("/assets/controller-shell.js".to_owned())
}

#[cfg(not(debug_assertions))]
pub async fn static_asset_path(
    test_assets: &TestAssets,
) -> Result<String, Box<dyn Error + Send + Sync>> {
    let response = app_router(&test_assets.assets)
        .oneshot(Request::get("/").body(Body::empty())?)
        .await?;
    let body = to_bytes(response.into_body(), 1024 * 1024).await?;
    let index = std::str::from_utf8(&body)?;
    let marker = "src=\"/assets/";
    let start = index
        .find(marker)
        .map(|position| position + "src=\"".len())
        .ok_or_else(|| std::io::Error::other("embedded index has no script asset"))?;
    let end = index[start..]
        .find('"')
        .map(|position| start + position)
        .ok_or_else(|| std::io::Error::other("embedded script asset is unterminated"))?;
    Ok(index[start..end].to_owned())
}

pub async fn asset_response(
    test_assets: &TestAssets,
    path: &str,
) -> Result<AssetResponse, Box<dyn Error + Send + Sync>> {
    let response = app_router(&test_assets.assets)
        .oneshot(Request::get(path).body(Body::empty())?)
        .await?;
    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .cloned()
        .ok_or_else(|| std::io::Error::other("asset response is missing content type"))?;
    let body = to_bytes(response.into_body(), 1024 * 1024).await?;
    Ok(AssetResponse {
        status,
        content_type,
        body,
    })
}

#[cfg(not(debug_assertions))]
pub fn test_assets() -> Result<TestAssets, Box<dyn Error + Send + Sync>> {
    Ok(TestAssets {
        assets: FrontendAssets::embedded()?,
        spa_marker: b"Videnoa Controller",
    })
}

pub async fn assert_api_not_found(method: Method, uri: &'static str) -> TestResult {
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

pub async fn assert_spa_get_response(response: Response, marker: &'static [u8]) -> TestResult {
    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .ok_or_else(|| std::io::Error::other("SPA response is missing content type"))?;
    assert_eq!(content_type, &HeaderValue::from_static("text/html"));
    let body = to_bytes(response.into_body(), 1024 * 1024).await?;
    assert!(body.windows(marker.len()).any(|part| part == marker));
    Ok(())
}

pub async fn assert_spa_head_response(response: Response) -> TestResult {
    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .ok_or_else(|| std::io::Error::other("SPA response is missing content type"))?;
    assert_eq!(content_type, &HeaderValue::from_static("text/html"));
    let body = to_bytes(response.into_body(), 1024).await?;
    assert!(body.is_empty());
    Ok(())
}

pub async fn assert_spa_method_rejected(method: Method) -> TestResult {
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

pub async fn assert_invalid_path_rejected(uri: &'static str) -> TestResult {
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
    let body = to_bytes(response.into_body(), 1024).await?;
    assert!(body.is_empty());
    Ok(())
}
