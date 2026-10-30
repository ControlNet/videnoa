//! Graphs that `/api/jobs` accepts must be runnable, and graphs that cannot run
//! must be rejected at submission.
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tempfile::TempDir;
use tower::ServiceExt;
use videnoa_core::{
    config::AppConfig,
    server::{api_router, app_state_with_config},
};

fn app(dir: &TempDir) -> Router {
    api_router(app_state_with_config(
        AppConfig::default(),
        dir.path().join("config.toml"),
        dir.path().join("data"),
    ))
}

async fn request(app: &Router, method: &str, uri: &str, payload: Value) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(payload.to_string()))
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

async fn terminal(app: &Router, id: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            let (_, job) = request(app, "GET", &format!("/api/jobs/{id}"), Value::Null).await;
            if matches!(
                job["status"].as_str(),
                Some("completed" | "failed" | "cancelled")
            ) {
                return job;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap()
}

/// A ten-frame 32x32 synthetic clip, or `None` when FFmpeg is unavailable.
fn synthetic_video(dir: &Path) -> Option<PathBuf> {
    let path = dir.join("input.mkv");
    let status = Command::new("ffmpeg")
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
            "yuv420p",
            "-c:v",
            "ffv1",
        ])
        .arg(&path)
        .status()
        .ok()?;
    status.success().then_some(path)
}

/// `VideoInput.source_path -> PathDivider -> PathJoiner -> VideoOutput.output_path`
/// derives the output path from the input path, a natural editor workflow.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scalar_node_downstream_of_frame_chain_node_runs() {
    let dir = TempDir::new().unwrap();
    let Some(input) = synthetic_video(dir.path()) else {
        eprintln!("skipping: ffmpeg is not available");
        return;
    };
    let app = app(&dir);
    let workflow = json!({
        "nodes": [
            {"id": "input", "node_type": "VideoInput", "params": {"path": input}},
            {"id": "div", "node_type": "PathDivider", "params": {}},
            {"id": "join", "node_type": "PathJoiner", "params": {"file_name": "derived.mp4"}},
            {"id": "output", "node_type": "VideoOutput", "params": {"codec": "libx264", "pixel_format": "yuv420p"}}
        ],
        "connections": [
            {"from_node": "input", "from_port": "frames", "to_node": "output", "to_port": "frames", "port_type": "VideoFrames"},
            {"from_node": "input", "from_port": "source_path", "to_node": "output", "to_port": "source_path", "port_type": "Path"},
            {"from_node": "input", "from_port": "source_path", "to_node": "div", "to_port": "path", "port_type": "Path"},
            {"from_node": "div", "from_port": "parent_path", "to_node": "join", "to_port": "parent_path", "port_type": "Path"},
            {"from_node": "join", "from_port": "path", "to_node": "output", "to_port": "output_path", "port_type": "Path"}
        ]
    });

    let (status, created) = request(&app, "POST", "/api/jobs", json!({"workflow": workflow})).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let job = terminal(&app, created["id"].as_str().unwrap()).await;

    assert_eq!(job["status"], "completed", "{job:#}");
    assert!(dir.path().join("derived.mp4").is_file());
}

/// A Workflow node runs its target with the scalar executor, which cannot
/// stream frames, so a target with a frame chain must be rejected up front.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nested_video_workflow_is_rejected_at_submission() {
    let dir = TempDir::new().unwrap();
    let inner = json!({
        "nodes": [
            {"id": "wi", "node_type": "WorkflowInput", "params": {"ports": [{"name": "input", "port_type": "Path"}]}},
            {"id": "input", "node_type": "VideoInput", "params": {}},
            {"id": "output", "node_type": "VideoOutput", "params": {"output_path": dir.path().join("nested-out.mp4")}},
            {"id": "wo", "node_type": "WorkflowOutput", "params": {"ports": [{"name": "output_path", "port_type": "Path"}]}}
        ],
        "connections": [
            {"from_node": "wi", "from_port": "input", "to_node": "input", "to_port": "path", "port_type": "Path"},
            {"from_node": "input", "from_port": "frames", "to_node": "output", "to_port": "frames", "port_type": "VideoFrames"},
            {"from_node": "input", "from_port": "source_path", "to_node": "output", "to_port": "source_path", "port_type": "Path"},
            {"from_node": "output", "from_port": "output_path", "to_node": "wo", "to_port": "output_path", "port_type": "Path"}
        ]
    });
    let inner_path = dir.path().join("inner.json");
    std::fs::write(&inner_path, serde_json::to_vec_pretty(&inner).unwrap()).unwrap();
    let app = app(&dir);
    let outer = json!({
        "nodes": [
            {"id": "c", "node_type": "Constant", "params": {"type": "Path", "value": dir.path().join("input.mkv")}},
            {"id": "wf", "node_type": "Workflow", "params": {
                "workflow_path": inner_path,
                "interface_inputs": [{"name": "input", "port_type": "Path"}],
                "interface_outputs": [{"name": "output_path", "port_type": "Path"}]
            }}
        ],
        "connections": [
            {"from_node": "c", "from_port": "value", "to_node": "wf", "to_port": "input", "port_type": "Path"}
        ]
    });

    let (status, body) = request(&app, "POST", "/api/jobs", json!({"workflow": outer})).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let message = body.to_string();
    assert!(message.contains("node 'wf'"), "{message}");
    assert!(message.contains("VideoFrames"), "{message}");
}
