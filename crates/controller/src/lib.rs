use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;

#[cfg(debug_assertions)]
use std::path::Path;

use axum::extract::{Extension, Request};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use axum::Json;
use axum::Router;
use percent_encoding::percent_decode_str;

mod asset_path;
pub mod auth;
pub mod config;
pub mod domain;
pub mod paths;
pub mod persistence;
pub mod tasks;
use asset_path::ExactAssetPath;
pub use auth::{
    authenticated_app_router, controller_app_router, serve_authenticated, serve_controller,
};

#[cfg(not(debug_assertions))]
use rust_embed::RustEmbed;

#[derive(Debug, thiserror::Error)]
pub enum StartupError {
    #[error("Controller frontend directory is missing: {path}")]
    FrontendDirectoryMissing { path: PathBuf },
    #[error("Controller frontend index is missing: {path}")]
    FrontendIndexMissing { path: PathBuf },
    #[error("Controller release assets do not contain index.html")]
    EmbeddedIndexMissing,
    #[error("failed to bind Controller HTTP listener at {address}")]
    Bind {
        address: SocketAddr,
        #[source]
        source: io::Error,
    },
    #[error("Controller HTTP server failed")]
    Serve(#[source] io::Error),
}

#[derive(Clone, Debug)]
pub struct FrontendAssets {
    source: FrontendAssetSource,
}

#[derive(Clone, Debug)]
enum FrontendAssetSource {
    #[cfg(debug_assertions)]
    Directory(PathBuf),
    #[cfg(not(debug_assertions))]
    Embedded,
}

impl FrontendAssets {
    /// Opens a generated frontend directory for debug-mode static serving.
    ///
    /// # Errors
    /// Returns a typed startup error when the directory or its SPA index is missing.
    #[cfg(debug_assertions)]
    pub fn from_dist(directory: impl AsRef<Path>) -> Result<Self, StartupError> {
        let directory = directory.as_ref();
        if !directory.is_dir() {
            return Err(StartupError::FrontendDirectoryMissing {
                path: directory.to_path_buf(),
            });
        }

        let index_path = directory.join("index.html");
        if !index_path.is_file() {
            return Err(StartupError::FrontendIndexMissing { path: index_path });
        }

        Ok(Self {
            source: FrontendAssetSource::Directory(directory.to_path_buf()),
        })
    }

    /// Verifies that the release binary contains the generated frontend index.
    ///
    /// # Errors
    /// Returns [`StartupError::EmbeddedIndexMissing`] when release assets were not embedded.
    #[cfg(not(debug_assertions))]
    pub fn embedded() -> Result<Self, StartupError> {
        if EmbeddedFrontend::get("index.html").is_none() {
            return Err(StartupError::EmbeddedIndexMissing);
        }

        Ok(Self {
            source: FrontendAssetSource::Embedded,
        })
    }
}

#[derive(serde::Serialize)]
struct ApiErrorResponse {
    error: &'static str,
}

async fn health() -> Json<domain::HealthResponse> {
    Json(domain::HealthResponse {
        status: domain::HealthStatus::Ok,
    })
}

async fn api_route_not_found() -> (StatusCode, Json<ApiErrorResponse>) {
    (
        StatusCode::NOT_FOUND,
        Json(ApiErrorResponse { error: "not_found" }),
    )
}

#[derive(Clone, Debug)]
struct DecodedPath(Box<str>);

impl DecodedPath {
    fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PathParseError {
    MalformedPercentEncoding,
    InvalidUtf8,
    UnsafeCharacter,
    AmbiguousSegment,
}

async fn parse_request_path(mut request: Request, next: Next) -> Response {
    let raw_path = request.uri().path();
    let decoded_path = match decode_request_path(raw_path) {
        Ok(decoded_path) => decoded_path,
        Err(
            PathParseError::MalformedPercentEncoding
            | PathParseError::InvalidUtf8
            | PathParseError::UnsafeCharacter
            | PathParseError::AmbiguousSegment,
        ) => return StatusCode::BAD_REQUEST.into_response(),
    };

    if decoded_path.as_str() != raw_path && is_api_path(decoded_path.as_str()) {
        return api_route_not_found().await.into_response();
    }

    request.extensions_mut().insert(decoded_path);
    next.run(request).await
}

fn decode_request_path(raw_path: &str) -> Result<DecodedPath, PathParseError> {
    let bytes = raw_path.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let Some(encoded_byte) = bytes.get(index + 1..index + 3) else {
                return Err(PathParseError::MalformedPercentEncoding);
            };
            if !encoded_byte.iter().all(u8::is_ascii_hexdigit) {
                return Err(PathParseError::MalformedPercentEncoding);
            }
            index += 3;
        } else {
            index += 1;
        }
    }

    let decoded_path = percent_decode_str(raw_path)
        .decode_utf8()
        .map_err(|_| PathParseError::InvalidUtf8)?;
    if decoded_path
        .chars()
        .any(|character| character == '\\' || character.is_control())
    {
        return Err(PathParseError::UnsafeCharacter);
    }

    let path_without_root = decoded_path
        .strip_prefix('/')
        .ok_or(PathParseError::AmbiguousSegment)?;
    if !path_without_root.is_empty() {
        let segments_path = path_without_root
            .strip_suffix('/')
            .unwrap_or(path_without_root);
        if segments_path.is_empty()
            || segments_path
                .split('/')
                .any(|segment| segment.is_empty() || segment == "." || segment == "..")
        {
            return Err(PathParseError::AmbiguousSegment);
        }
    }

    Ok(DecodedPath(decoded_path.into_owned().into_boxed_str()))
}

fn is_api_path(path: &str) -> bool {
    path == "/api" || path.starts_with("/api/")
}

#[cfg(not(debug_assertions))]
#[derive(RustEmbed)]
#[folder = "../../controller-web/dist/"]
struct EmbeddedFrontend;

#[cfg(not(debug_assertions))]
async fn embedded_static(Extension(decoded_path): Extension<DecodedPath>) -> Response {
    if let Some(asset_path) = ExactAssetPath::from_decoded_path(decoded_path.as_str()) {
        let asset_path = asset_path.as_str();
        if let Some(asset) = EmbeddedFrontend::get(asset_path) {
            let content_type = mime_guess::from_path(asset_path).first_or_octet_stream();
            return (
                [(header::CONTENT_TYPE, content_type.essence_str())],
                asset.data.into_owned(),
            )
                .into_response();
        }
    }

    match EmbeddedFrontend::get("index.html") {
        Some(index) => (
            [(header::CONTENT_TYPE, "text/html")],
            index.data.into_owned(),
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(debug_assertions)]
async fn directory_static(
    Extension(decoded_path): Extension<DecodedPath>,
    Extension(directory): Extension<PathBuf>,
) -> Response {
    if let Some(asset_path) = ExactAssetPath::from_decoded_path(decoded_path.as_str()) {
        if let Ok(asset) = tokio::fs::read(asset_path.join_to(&directory)).await {
            let content_type = mime_guess::from_path(asset_path.as_str()).first_or_octet_stream();
            return ([(header::CONTENT_TYPE, content_type.essence_str())], asset).into_response();
        }
    }

    match tokio::fs::read(directory.join("index.html")).await {
        Ok(index) => ([(header::CONTENT_TYPE, "text/html")], index).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

pub fn app_router(assets: &FrontendAssets) -> Router {
    let router = Router::new()
        .route("/api/health", get(health))
        .route("/api", any(api_route_not_found))
        .route("/api/", any(api_route_not_found))
        .route("/api/{*path}", any(api_route_not_found));

    let router = match &assets.source {
        #[cfg(debug_assertions)]
        FrontendAssetSource::Directory(directory) => {
            router.fallback_service(get(directory_static).layer(Extension(directory.clone())))
        }
        #[cfg(not(debug_assertions))]
        FrontendAssetSource::Embedded => router.fallback_service(get(embedded_static)),
    };

    router.layer(middleware::from_fn(parse_request_path))
}

/// Serves the Controller API and frontend until the HTTP server exits.
///
/// # Errors
/// Returns a typed startup error when the listener cannot bind or the server fails.
pub async fn serve(address: SocketAddr, assets: &FrontendAssets) -> Result<(), StartupError> {
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(|source| StartupError::Bind { address, source })?;

    axum::serve(listener, app_router(assets))
        .await
        .map_err(StartupError::Serve)
}
