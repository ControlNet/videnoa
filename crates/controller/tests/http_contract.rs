use axum::body::{to_bytes, Body};
use axum::http::{header, Method, Request, StatusCode};
use tower::ServiceExt;
use videnoa_controller::{app_router, FrontendAssets};

mod support;
use support::{
    assert_api_not_found, assert_invalid_path_rejected, assert_spa_get_response,
    assert_spa_head_response, assert_spa_method_rejected, asset_response, spa_response_body,
    static_asset_path, test_assets, TestResult,
};

#[cfg(debug_assertions)]
use std::fs;
#[cfg(debug_assertions)]
use tempfile::TempDir;
#[cfg(debug_assertions)]
use videnoa_controller::StartupError;

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
async fn safely_encoded_static_assets_match_canonical_asset() -> TestResult {
    // Given: a real generated or fixture asset whose filename contains a hyphen.
    let test_assets = test_assets()?;
    let canonical_path = static_asset_path(&test_assets).await?;
    let encoded_hyphen_path = canonical_path.replace('-', "%2D");
    let encoded_slash_path = canonical_path.replacen("/assets/", "/assets%2F", 1);
    assert_ne!(encoded_hyphen_path, canonical_path);
    assert_ne!(encoded_slash_path, canonical_path);

    // When: canonical and safely encoded asset paths are requested.
    let canonical = asset_response(&test_assets, &canonical_path).await?;
    let encoded_hyphen = asset_response(&test_assets, &encoded_hyphen_path).await?;
    let encoded_slash = asset_response(&test_assets, &encoded_slash_path).await?;
    let spa_body = spa_response_body(&test_assets).await?;

    // Then: every spelling returns the expected non-HTML asset rather than the SPA index.
    assert_eq!(canonical.status, StatusCode::OK);
    assert_eq!(canonical.content_type, "text/javascript");
    assert!(!canonical.body.is_empty());
    assert_ne!(canonical.body, spa_body);
    for encoded in [encoded_hyphen, encoded_slash] {
        assert_eq!(encoded.status, canonical.status);
        assert_eq!(encoded.content_type, canonical.content_type);
        assert_eq!(encoded.body, canonical.body);
    }
    #[cfg(debug_assertions)]
    assert_eq!(canonical.body, "console.info('encoded fixture asset')");
    Ok(())
}

#[tokio::test]
async fn ambiguous_dot_and_empty_segments_return_bad_request() -> TestResult {
    // Given: a real asset addressed through literal/encoded dot segments and repeated separators.
    let test_assets = test_assets()?;
    let canonical_path = static_asset_path(&test_assets).await?;
    let filename = canonical_path
        .strip_prefix("/assets/")
        .ok_or_else(|| std::io::Error::other("asset path is outside /assets"))?;
    let ambiguous_paths = [
        format!("/assets/%2e/{filename}"),
        format!("/%2e/assets/{filename}"),
        format!("/assets/./{filename}"),
        format!("/./assets/{filename}"),
        format!("/assets/%2e%2e/{filename}"),
        format!("/%2e%2e/assets/{filename}"),
        format!("/assets/../assets/{filename}"),
        format!("/../assets/{filename}"),
        format!("/assets//{filename}"),
    ];

    // When: each ambiguous path reaches the decoded boundary.
    for path in ambiguous_paths {
        assert_invalid_path_rejected(&path).await?;
    }

    // Then: every profile returns the same empty non-HTML 400 outcome.
    Ok(())
}

#[tokio::test]
async fn decoded_backslash_and_control_paths_return_bad_request() -> TestResult {
    // Given: encoded backslashes, NUL, and another C0 control byte.
    let invalid_paths = ["/assets%5Capp.js", "/assets%5capp.js", "/%00", "/%1F"];

    // When: each encoded path reaches the Controller boundary.
    for path in invalid_paths {
        assert_invalid_path_rejected(path).await?;
    }

    // Then: every response is an empty non-HTML 400.
    Ok(())
}

#[tokio::test]
async fn double_encoded_api_separator_is_not_recursively_decoded() -> TestResult {
    // Given: a path that becomes encoded API syntax after exactly one decode.
    let test_assets = test_assets()?;

    // When: the double-encoded path is requested.
    let response = app_router(&test_assets.assets)
        .oneshot(Request::get("/api%252Funknown").body(Body::empty())?)
        .await?;

    // Then: it remains a client route and returns the actual SPA body.
    assert_spa_get_response(response, test_assets.spa_marker).await
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
async fn ordinary_unicode_query_and_trailing_slash_routes_return_spa() -> TestResult {
    // Given: harmless client routes containing Unicode, a dot in the query, or a trailing slash.
    let test_assets = test_assets()?;
    let routes = [
        "/tasks/%E6%97%A5%E6%9C%AC%E8%AA%9E",
        "/tasks/example?view=.",
        "/tasks/example/",
    ];

    // When: each ordinary client route is requested.
    for route in routes {
        let response = app_router(&test_assets.assets)
            .oneshot(Request::get(route).body(Body::empty())?)
            .await?;
        assert_spa_get_response(response, test_assets.spa_marker).await?;
    }

    // Then: path parsing preserves legitimate routes and ignores query-string punctuation.
    Ok(())
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
