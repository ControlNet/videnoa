use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;

#[cfg(debug_assertions)]
use std::path::Path;

use axum::http::StatusCode;
use axum::routing::{any, get};
use axum::Json;
use axum::Router;
use serde::Serialize;

#[cfg(not(debug_assertions))]
use axum::extract::OriginalUri;
#[cfg(not(debug_assertions))]
use axum::http::header;
#[cfg(not(debug_assertions))]
use axum::response::{IntoResponse, Response};
#[cfg(not(debug_assertions))]
use rust_embed::RustEmbed;
#[cfg(debug_assertions)]
use tower_http::services::{ServeDir, ServeFile};

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

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[derive(Serialize)]
struct ApiErrorResponse {
    error: &'static str,
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn api_route_not_found() -> (StatusCode, Json<ApiErrorResponse>) {
    (
        StatusCode::NOT_FOUND,
        Json(ApiErrorResponse { error: "not_found" }),
    )
}

#[cfg(not(debug_assertions))]
#[derive(RustEmbed)]
#[folder = "../../controller-web/dist/"]
struct EmbeddedFrontend;

#[cfg(not(debug_assertions))]
async fn embedded_static(OriginalUri(uri): OriginalUri) -> Response {
    let requested_path = uri.path().trim_start_matches('/');
    let asset_path = if requested_path.is_empty() {
        "index.html"
    } else {
        requested_path
    };

    if let Some(asset) = EmbeddedFrontend::get(asset_path) {
        let content_type = mime_guess::from_path(asset_path).first_or_octet_stream();
        return (
            [(header::CONTENT_TYPE, content_type.essence_str())],
            asset.data.into_owned(),
        )
            .into_response();
    }

    match EmbeddedFrontend::get("index.html") {
        Some(index) => (
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            index.data.into_owned(),
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

pub fn app_router(assets: &FrontendAssets) -> Router {
    let router = Router::new()
        .route("/api/health", get(health))
        .route("/api", any(api_route_not_found))
        .route("/api/", any(api_route_not_found))
        .route("/api/{*path}", any(api_route_not_found));

    match &assets.source {
        #[cfg(debug_assertions)]
        FrontendAssetSource::Directory(directory) => {
            let index_path = directory.join("index.html");
            router.fallback_service(
                ServeDir::new(directory.clone()).fallback(ServeFile::new(index_path)),
            )
        }
        #[cfg(not(debug_assertions))]
        FrontendAssetSource::Embedded => router.fallback(embedded_static),
    }
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
