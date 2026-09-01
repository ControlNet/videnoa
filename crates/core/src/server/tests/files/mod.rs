use super::*;

use axum::body::Body;
use axum::http::{Method, Request};
use axum::response::Response;
use tempfile::TempDir;
use tower::ServiceExt;

mod lifecycle;
mod security;

fn test_app(data_dir: &StdPath) -> Router {
    test_app_with_models(data_dir, &data_dir.join("models"))
}

fn test_app_with_models(data_dir: &StdPath, models_dir: &StdPath) -> Router {
    let config = AppConfig {
        paths: crate::config::PathsConfig {
            models_dir: models_dir.to_path_buf(),
            trt_cache_dir: data_dir.join("trt-cache"),
            presets_dir: data_dir.join("presets"),
            workflows_dir: data_dir.join("workflows"),
        },
        ..AppConfig::default()
    };
    let state = AppState::new(
        NodeRegistry::new(),
        ModelRegistry::new(models_dir.to_path_buf()),
        DashMap::new(),
        config,
        data_dir.join("config.toml"),
        data_dir.to_path_buf(),
    );

    app_router(state)
}

async fn send(app: &Router, method: Method, uri: &str, body: Body) -> Response {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .body(body)
        .expect("build file API request");

    app.clone()
        .oneshot(request)
        .await
        .expect("file API response")
}

async fn body_bytes(response: Response) -> axum::body::Bytes {
    axum::body::to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .expect("read response body")
}

fn temp_dir() -> TempDir {
    tempfile::tempdir().expect("create file API temp directory")
}

fn workspace_path(data_dir: &StdPath, relative: &str) -> PathBuf {
    data_dir.join("workspace").join(relative)
}

fn create_workspace_file(data_dir: &StdPath, relative: &str, bytes: &[u8]) {
    let path = workspace_path(data_dir, relative);
    std::fs::create_dir_all(path.parent().expect("workspace file parent"))
        .expect("create workspace file parent");
    std::fs::write(path, bytes).expect("write workspace fixture");
}
