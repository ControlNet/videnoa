use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    routing::get,
    Router,
};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tempfile::TempDir;
use tokio::sync::Notify;
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
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let (_, job) = request(app, "GET", &format!("/api/jobs/{id}"), Value::Null).await;
            if matches!(
                job["status"].as_str(),
                Some("completed" | "failed" | "cancelled")
            ) {
                return job;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

// The local HTTP service and paths below are explicit synthetic test fixtures.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn inferred_path_parameter_preserves_declared_type() {
    let dir = TempDir::new().unwrap();
    let app = app(&dir);
    let mut workflow = json!({"nodes":[
        {"id":"input","node_type":"WorkflowInput","params":{"ports":[{"name":"input_path","port_type":"Path"}],"input_path":"/tmp/review-fixture.mkv"}},
        {"id":"divide","node_type":"PathDivider","params":{}}
    ],"connections":[{"from_node":"input","from_port":"input_path","to_node":"divide","to_port":"path","port_type":"Path"}]});
    let (status, created) = request(&app, "POST", "/api/jobs", json!({"workflow":workflow})).await;
    assert_eq!(status, StatusCode::CREATED);
    let completed = terminal(&app, created["id"].as_str().unwrap()).await;
    assert_eq!(completed["status"], "completed", "{completed}");
    let (_, control) = request(
        &app,
        "POST",
        "/api/jobs",
        json!({"workflow":workflow,"params":{}}),
    )
    .await;
    assert_eq!(
        terminal(&app, control["id"].as_str().unwrap()).await["status"],
        "completed"
    );
    workflow["nodes"][0]["params"]
        .as_object_mut()
        .unwrap()
        .remove("input_path");
    let (status, explicit) = request(
        &app,
        "POST",
        "/api/jobs",
        json!({"workflow":workflow,"params":{"input_path":"/tmp/explicit-fixture.mkv"}}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let completed = terminal(&app, explicit["id"].as_str().unwrap()).await;
    assert_eq!(completed["status"], "completed", "{completed}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deleted_scalar_job_does_not_execute_downstream_http_request() {
    for nested in [false, true] {
        let dir = TempDir::new().unwrap();
        let app = app(&dir);
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let downstream = Arc::new(Notify::new());
        let endpoint = Router::new()
            .route(
                "/first",
                get({
                    let entered = entered.clone();
                    let release = release.clone();
                    move || {
                        let entered = entered.clone();
                        let release = release.clone();
                        async move {
                            entered.notify_one();
                            release.notified().await;
                            "test response"
                        }
                    }
                }),
            )
            .route(
                "/second",
                get({
                    let downstream = downstream.clone();
                    move || {
                        let downstream = downstream.clone();
                        async move {
                            downstream.notify_one();
                            "test downstream"
                        }
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, endpoint).await.unwrap();
        });
        let workflow = json!({"nodes":[
        {"id":"first","node_type":"HttpRequest","params":{"url":format!("{base}/first"),"max_retries":0}},
        {"id":"second","node_type":"HttpRequest","params":{"url":format!("{base}/second"),"max_retries":0}}
    ],"connections":[{"from_node":"first","from_port":"response_body","to_node":"second","to_port":"body","port_type":"Str"}]});
        let workflow = if nested {
            let path = dir.path().join("nested.json");
            std::fs::write(&path, workflow.to_string()).unwrap();
            json!({"nodes":[{"id":"nested","node_type":"Workflow","params":{"workflow_path":path}}],"connections":[]})
        } else {
            workflow
        };
        let (status, created) =
            request(&app, "POST", "/api/jobs", json!({"workflow":workflow})).await;
        assert_eq!(status, StatusCode::CREATED);
        tokio::time::timeout(Duration::from_secs(10), entered.notified())
            .await
            .unwrap();
        let uri = format!("/api/jobs/{}", created["id"].as_str().unwrap());
        assert_eq!(
            request(&app, "DELETE", &uri, Value::Null).await.0,
            StatusCode::NO_CONTENT
        );
        release.notify_one();
        let (_, next) = request(
            &app,
            "POST",
            "/api/jobs",
            json!({"workflow":{"nodes":[],"connections":[]}}),
        )
        .await;
        assert_eq!(
            terminal(&app, next["id"].as_str().unwrap()).await["status"],
            "completed"
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(50), downstream.notified())
                .await
                .is_err()
        );
        assert_eq!(
            request(&app, "GET", &uri, Value::Null).await.0,
            StatusCode::NOT_FOUND
        );
        server.abort();
    }
}

fn preview_workflow(kind: &str, params: Value) -> Value {
    json!({"nodes":[
        {"id":"source","node_type":"VideoInput","params":{}},
        {"id":"processor","node_type":kind,"params":params},
        {"id":"sink","node_type":"VideoOutput","params":{"output_path":"must-not-be-written.mkv"}}
    ],"connections":[
        {"from_node":"source","from_port":"frames","to_node":"processor","to_port":"frames","port_type":"VideoFrames"},
        {"from_node":"processor","from_port":"frames","to_node":"sink","to_port":"frames","port_type":"VideoFrames"}
    ]})
}

async fn extracted_preview(app: &Router, dir: &TempDir) -> Option<Value> {
    if std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_err()
    {
        eprintln!("Skipping media integration test: FFmpeg unavailable");
        return None;
    }
    // A generated test pattern is synthetic media used only by this test.
    let video = dir.path().join("fixture.mkv");
    let status = std::process::Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=32x32:rate=1",
            "-frames:v",
            "1",
        ])
        .arg(&video)
        .status()
        .unwrap();
    assert!(status.success());
    let (status, extracted) = request(
        app,
        "POST",
        "/api/preview/extract",
        json!({"video_path":video,"count":1}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{extracted}");
    Some(extracted)
}

fn remove_preview(extracted: &Value) {
    std::fs::remove_dir_all(std::env::temp_dir().join(format!(
        "videnoa-preview-{}",
        extracted["preview_id"].as_str().unwrap()
    )))
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preview_runs_resize_and_keeps_original_unchanged() {
    let dir = TempDir::new().unwrap();
    let app = app(&dir);
    let Some(extracted) = extracted_preview(&app, &dir).await else {
        return;
    };
    let original_url = extracted["frames"][0]["url"].as_str().unwrap();
    let original = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(original_url)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let original = to_bytes(original.into_body(), 1024 * 1024).await.unwrap();
    let workflow = preview_workflow(
        "Resize",
        json!({"width":16,"height":8,"algorithm":"nearest"}),
    );
    let payload = json!({"preview_id":extracted["preview_id"],"frame_index":0,"workflow":workflow});
    let (status, processed) = request(&app, "POST", "/api/preview/process", payload.clone()).await;
    assert_eq!(status, StatusCode::OK, "{processed}");
    assert_ne!(processed["processed_url"], original_url);
    let image = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(processed["processed_url"].as_str().unwrap())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(image.status(), StatusCode::OK);
    let bytes = to_bytes(image.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(u32::from_be_bytes(bytes[16..20].try_into().unwrap()), 16);
    assert_eq!(u32::from_be_bytes(bytes[20..24].try_into().unwrap()), 8);
    let (_, again) = request(&app, "POST", "/api/preview/process", payload).await;
    assert_ne!(again["processed_url"], processed["processed_url"]);
    let untouched = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(original_url)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        to_bytes(untouched.into_body(), 1024 * 1024).await.unwrap(),
        original
    );
    assert!(!std::path::Path::new("must-not-be-written.mkv").exists());
    remove_preview(&extracted);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preview_rejects_invalid_temporal_and_external_side_effect_workflows() {
    let dir = TempDir::new().unwrap();
    let app = app(&dir);
    let Some(extracted) = extracted_preview(&app, &dir).await else {
        return;
    };
    for kind in ["UnknownNode", "FrameInterpolation", "HttpRequest"] {
        let workflow = preview_workflow(kind, json!({"url":"http://127.0.0.1:1/should-not-run"}));
        let (status, response) = request(
            &app,
            "POST",
            "/api/preview/process",
            json!({"preview_id":extracted["preview_id"],"frame_index":0,"workflow":workflow}),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{kind}: {response}");
    }
    let (status, _) = request(
        &app,
        "POST",
        "/api/preview/process",
        json!({"preview_id":extracted["preview_id"],"frame_index":u32::MAX,"workflow":{}}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    remove_preview(&extracted);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires CUDA, ONNX Runtime and the local RealESRGAN model"]
async fn preview_superresolution_cuda_produces_upscaled_png() {
    let dir = TempDir::new().unwrap();
    let app = app(&dir);
    let extracted = extracted_preview(&app, &dir)
        .await
        .expect("FFmpeg required for GPU test");
    let model = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../models/RealESRGAN_x4plus_anime_6B.onnx");
    let preset: Value =
        serde_json::from_str(include_str!("../../../presets/anime-4x-upscale.json")).unwrap();
    let mut workflow = preset["workflow"].clone();
    let processor = workflow["nodes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|node| node["node_type"] == "SuperResolution")
        .unwrap();
    processor["params"]["model_path"] = json!(model);
    processor["params"]["backend"] = json!("cuda");
    processor["params"]["tile_size"] = json!(0);
    let (status, processed) = request(
        &app,
        "POST",
        "/api/preview/process",
        json!({"preview_id":extracted["preview_id"],"frame_index":0,"workflow":workflow}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{processed}");
    let image = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(processed["processed_url"].as_str().unwrap())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = to_bytes(image.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(u32::from_be_bytes(bytes[16..20].try_into().unwrap()), 128);
    assert_eq!(u32::from_be_bytes(bytes[20..24].try_into().unwrap()), 128);
    remove_preview(&extracted);
}
