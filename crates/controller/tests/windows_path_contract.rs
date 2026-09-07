use std::error::Error;

use axum::body::{to_bytes, Body, Bytes};
use axum::http::{header, HeaderValue, Request, StatusCode};
use tower::ServiceExt;
use videnoa_controller::{app_router, FrontendAssets};

#[cfg(debug_assertions)]
use std::fs;
#[cfg(debug_assertions)]
use tempfile::TempDir;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

struct TestAssets {
    #[cfg(debug_assertions)]
    _directory: TempDir,
    assets: FrontendAssets,
}

struct AssetResponse {
    status: StatusCode,
    content_type: HeaderValue,
    body: Bytes,
}

#[cfg(debug_assertions)]
fn test_assets() -> TestResult<TestAssets> {
    let directory = TempDir::new()?;
    fs::write(
        directory.path().join("index.html"),
        "<main>fixture shell</main>",
    )?;
    let assets = FrontendAssets::from_dist(directory.path())?;
    Ok(TestAssets {
        _directory: directory,
        assets,
    })
}

#[cfg(not(debug_assertions))]
fn test_assets() -> TestResult<TestAssets> {
    Ok(TestAssets {
        assets: FrontendAssets::embedded()?,
    })
}

async fn response(test_assets: &TestAssets, path: &str) -> TestResult<AssetResponse> {
    let response = app_router(&test_assets.assets)
        .oneshot(Request::get(path).body(Body::empty())?)
        .await?;
    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .cloned()
        .ok_or_else(|| std::io::Error::other("response is missing content type"))?;
    let body = to_bytes(response.into_body(), 1024 * 1024).await?;
    Ok(AssetResponse {
        status,
        content_type,
        body,
    })
}

async fn spa_body(test_assets: &TestAssets) -> TestResult<Bytes> {
    Ok(response(test_assets, "/").await?.body)
}

fn reserved_names() -> [&'static str; 30] {
    [
        "CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "COM1", "COM2", "COM3", "COM4", "COM5",
        "COM6", "COM7", "COM8", "COM9", "COM¹", "COM²", "COM³", "LPT1", "LPT2", "LPT3", "LPT4",
        "LPT5", "LPT6", "LPT7", "LPT8", "LPT9", "LPT¹", "LPT²", "LPT³",
    ]
}

#[tokio::test]
async fn drive_and_ads_spellings_return_spa() -> TestResult {
    // Given: portable frontend assets and Windows drive or ADS request spellings.
    let test_assets = test_assets()?;
    let spa_body = spa_body(&test_assets).await?;
    let paths = [
        "/C:/Windows/win.ini",
        "/C%3A/Windows/win.ini",
        "/%43%3a/Windows/win.ini",
        "/assets/file.txt:secret",
        "/assets/file.txt%3Asecret",
    ];

    // When: each unsafe-for-filesystem client path is requested.
    for path in paths {
        let response = response(&test_assets, path).await?;

        // Then: lookup is ineligible and the SPA handles the route.
        assert_eq!(response.status, StatusCode::OK);
        assert_eq!(response.content_type, "text/html");
        assert_eq!(response.body, spa_body);
    }
    Ok(())
}

#[tokio::test]
async fn windows_reserved_device_names_return_spa_for_normalized_variants() -> TestResult {
    // Given: every case-insensitive DOS device basename and normalized filename shape.
    let test_assets = test_assets()?;
    let spa_body = spa_body(&test_assets).await?;

    // When: bare, extension, nested, trailing-space, and trailing-dot variants are requested.
    for name in reserved_names() {
        for path in [
            format!("/assets/{name}"),
            format!("/assets/{}.txt", name.to_lowercase()),
            format!("/assets/{name}/nested"),
            format!("/assets/{name}%20"),
            format!("/assets/{name}."),
        ] {
            let response = response(&test_assets, &path).await?;

            // Then: every normalized device spelling returns the actual SPA body.
            assert_eq!(response.status, StatusCode::OK);
            assert_eq!(response.content_type, "text/html");
            assert_eq!(response.body, spa_body);
        }
    }
    Ok(())
}

#[cfg(all(debug_assertions, unix))]
#[tokio::test]
async fn unsafe_filesystem_fixtures_are_never_served() -> TestResult {
    // Given: Unix-hosted files whose names model unsafe Windows filesystem paths.
    let directory = TempDir::new()?;
    fs::write(
        directory.path().join("index.html"),
        "<main>fixture shell</main>",
    )?;
    fs::create_dir_all(directory.path().join("C:/Windows"))?;
    fs::create_dir(directory.path().join("assets"))?;
    fs::write(
        directory.path().join("C:/Windows/win.ini"),
        "unsafe filesystem fixture",
    )?;
    fs::write(
        directory.path().join("assets/file.txt:secret"),
        "unsafe filesystem fixture",
    )?;
    for name in reserved_names() {
        fs::write(
            directory.path().join("assets").join(name),
            "unsafe filesystem fixture",
        )?;
    }
    let assets = FrontendAssets::from_dist(directory.path())?;
    let spa_response = app_router(&assets)
        .oneshot(Request::get("/").body(Body::empty())?)
        .await?;
    let spa_body = to_bytes(spa_response.into_body(), 1024 * 1024).await?;

    // When: representative unsafe paths target files that really exist on the host.
    for path in [
        "/C:/Windows/win.ini",
        "/assets/file.txt:secret",
        "/assets/COM¹",
        "/assets/CONOUT$",
    ] {
        let response = app_router(&assets)
            .oneshot(Request::get(path).body(Body::empty())?)
            .await?;
        let body = to_bytes(response.into_body(), 1024 * 1024).await?;

        // Then: the actual SPA replaces every unsafe fixture.
        assert_eq!(body, spa_body);
        assert!(!body
            .windows(25)
            .any(|part| part == b"unsafe filesystem fixture"));
    }
    Ok(())
}
