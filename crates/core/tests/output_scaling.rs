//! Output scaling in video jobs: trailing Resize/Rescale nodes and VideoOutput
//! width/height are applied by the FFmpeg encoder (issue #5).
#![cfg(unix)]
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
use tempfile::TempDir;
use tower::ServiceExt;
use videnoa_core::{
    config::AppConfig,
    server::{api_router, app_state_with_config},
};

const FFMPEG: &str = "/usr/bin/ffmpeg";
const FFPROBE: &str = "/usr/bin/ffprobe";

fn media_tools_available() -> bool {
    let available = Path::new(FFMPEG).exists() && Path::new(FFPROBE).exists();
    if !available {
        eprintln!("Skipping output scaling test: system FFmpeg and FFprobe are required");
    }
    available
}

fn app(dir: &TempDir) -> Router {
    api_router(app_state_with_config(
        AppConfig::default(),
        dir.path().join("config.toml"),
        dir.path().join("data"),
    ))
}

async fn request(app: &Router, method: &str, uri: &str, value: Value) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(value.to_string()))
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

/// A 32x32, 10-frame lossless test clip in the given pixel format.
fn video(dir: &Path, pixel_format: &str) -> PathBuf {
    let path = dir.join(format!("input-{pixel_format}.mkv"));
    assert!(Command::new(FFMPEG)
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=32x32:rate=2",
            "-frames:v",
            "10",
            "-pix_fmt",
            pixel_format,
            "-c:v",
            "ffv1",
        ])
        .arg(&path)
        .status()
        .unwrap()
        .success());
    path
}

fn probe_dimensions(path: &Path) -> String {
    let output = Command::new(FFPROBE)
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .unwrap();
    assert!(output.status.success(), "ffprobe failed for {path:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn workflow(input: &Path, output: &Path, processing: &[Value], output_params: Value) -> Value {
    let mut params = json!({
        "output_path": output,
        "codec": "libx264",
        "pixel_format": "yuv420p",
        "x265_preset": "ultrafast",
    });
    params
        .as_object_mut()
        .unwrap()
        .extend(output_params.as_object().unwrap().clone());

    let mut nodes =
        vec![json!({"id": "input", "node_type": "VideoInput", "params": {"path": input}})];
    let mut connections = Vec::new();
    let mut previous = "input".to_string();
    for (index, node) in processing.iter().enumerate() {
        let id = format!("stage{index}");
        let mut node = node.clone();
        node["id"] = json!(id);
        nodes.push(node);
        connections.push(json!({"from_node": previous, "from_port": "frames", "to_node": id, "to_port": "frames", "port_type": "VideoFrames"}));
        previous = id;
    }
    nodes.push(json!({"id": "output", "node_type": "VideoOutput", "params": params}));
    connections.push(json!({"from_node": previous, "from_port": "frames", "to_node": "output", "to_port": "frames", "port_type": "VideoFrames"}));
    connections.push(json!({"from_node": "input", "from_port": "source_path", "to_node": "output", "to_port": "source_path", "port_type": "Path"}));
    json!({"nodes": nodes, "connections": connections})
}

async fn run_job(app: &Router, workflow: Value) -> Value {
    let (status, created) = request(app, "POST", "/api/jobs", json!({"workflow": workflow})).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            let (_, job) = request(
                app,
                "GET",
                &format!("/api/jobs/{}", created["id"].as_str().unwrap()),
                Value::Null,
            )
            .await;
            if matches!(
                job["status"].as_str(),
                Some("completed" | "failed" | "cancelled")
            ) {
                break job;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn trailing_rescale_scales_the_encoded_video() {
    if !media_tools_available() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let input = video(dir.path(), "yuv420p");
    let output = dir.path().join("rescaled.mkv");
    let app = app(&dir);

    let job = run_job(
        &app,
        workflow(
            &input,
            &output,
            &[json!({"node_type": "Rescale", "params": {"scale_factor": 0.5, "algorithm": "lanczos"}})],
            json!({}),
        ),
    )
    .await;

    assert_eq!(job["status"], "completed", "{job}");
    assert_eq!(probe_dimensions(&output), "16,16");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ten_bit_source_is_scaled_by_video_output_dimensions() {
    if !media_tools_available() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let input = video(dir.path(), "yuv420p10le");
    let output = dir.path().join("resized.mkv");
    let app = app(&dir);

    // 10-bit sources reach the encoder as 16-bit RGB when no model stage runs.
    let job = run_job(
        &app,
        workflow(&input, &output, &[], json!({"width": 24, "height": 16})),
    )
    .await;

    assert_eq!(job["status"], "completed", "{job}");
    assert_eq!(probe_dimensions(&output), "24,16");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn processing_after_rescale_is_rejected_before_the_job_is_queued() {
    let dir = TempDir::new().unwrap();
    let app = app(&dir);
    let input = dir.path().join("input.mkv");
    let output = dir.path().join("output.mkv");

    let (status, body) = request(
        &app,
        "POST",
        "/api/jobs",
        json!({"workflow": workflow(
            &input,
            &output,
            &[
                json!({"node_type": "Rescale", "params": {"scale_factor": 0.5}}),
                json!({"node_type": "SuperResolution", "params": {"model_path": dir.path().join("model.onnx"), "scale": 2}}),
            ],
            json!({}),
        )}),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let message = body.to_string();
    assert!(
        message.contains("'SuperResolution' cannot follow 'Rescale'"),
        "{message}"
    );
}
