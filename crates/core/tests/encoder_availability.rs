//! Isolated process: this test changes CWD so the job API resolves a synthetic
//! FFmpeg that lists only NVENC encoders, like the LGPL bundle from issue #6.
#![cfg(unix)]
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use tower::ServiceExt;
use videnoa_core::{
    config::AppConfig,
    server::{api_router, app_state_with_config},
};

const NVENC_ONLY_FFMPEG: &str = r#"#!/bin/sh
if [ "$2" = "-encoders" ]; then
  printf 'Encoders:\n V..... = Video\n ------\n V....D h264_nvenc           NVIDIA NVENC H.264 encoder (codec h264)\n V....D hevc_nvenc           NVIDIA NVENC hevc encoder (codec hevc)\n'
  exit 0
fi
exit 1
"#;

fn app(dir: &TempDir) -> Router {
    api_router(app_state_with_config(
        AppConfig::default(),
        dir.path().join("config.toml"),
        dir.path().join("data"),
    ))
}

async fn create_job(app: &Router, workflow: Value) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/jobs")
                .header("content-type", "application/json")
                .body(Body::from(json!({ "workflow": workflow }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn workflow(dir: &Path, output_params: Value) -> Value {
    let mut params = json!({"output_path": dir.join("output.mkv")});
    params
        .as_object_mut()
        .unwrap()
        .extend(output_params.as_object().unwrap().clone());
    json!({"nodes":[
        {"id":"input","node_type":"VideoInput","params":{"path":dir.join("input.mkv")}},
        {"id":"output","node_type":"VideoOutput","params":params}
    ],"connections":[
        {"from_node":"input","from_port":"frames","to_node":"output","to_port":"frames","port_type":"VideoFrames"},
        {"from_node":"input","from_port":"source_path","to_node":"output","to_port":"source_path","port_type":"Path"}
    ]})
}

struct CurrentDirectory(PathBuf);
impl Drop for CurrentDirectory {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.0).unwrap();
    }
}
fn enter(dir: &Path) -> CurrentDirectory {
    let previous = CurrentDirectory(std::env::current_dir().unwrap());
    std::env::set_current_dir(dir).unwrap();
    previous
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn job_with_an_encoder_missing_from_ffmpeg_is_rejected_before_it_is_queued() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new().unwrap();
    let ffmpeg = dir.path().join("bin").join("ffmpeg");
    std::fs::create_dir_all(ffmpeg.parent().unwrap()).unwrap();
    std::fs::write(&ffmpeg, NVENC_ONLY_FFMPEG).unwrap();
    std::fs::set_permissions(&ffmpeg, std::fs::Permissions::from_mode(0o700)).unwrap();
    let _cwd = enter(dir.path());
    let app = app(&dir);

    // The default VideoOutput codec is libx265.
    let (status, body) = create_job(&app, workflow(dir.path(), json!({}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let message = body.to_string();
    assert!(
        message.contains("VideoOutput 'output' uses encoder 'libx265'"),
        "{message}"
    );
    assert!(message.contains("hevc_nvenc, h264_nvenc"), "{message}");

    let (status, body) = create_job(
        &app,
        workflow(
            dir.path(),
            json!({"codec": "hevc_nvenc", "pixel_format": "p010le"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}
