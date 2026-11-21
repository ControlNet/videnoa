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
        // The cancelled job stays visible until it is deleted again.
        assert_eq!(
            terminal(&app, created["id"].as_str().unwrap()).await["status"],
            "cancelled"
        );
        assert_eq!(
            request(&app, "DELETE", &uri, Value::Null).await.0,
            StatusCode::NO_CONTENT
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

async fn remove_preview(app: &Router, extracted: &Value) {
    let (status, _) = request(
        app,
        "DELETE",
        &format!("/api/preview/{}", extracted["preview_id"].as_str().unwrap()),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = request(
        app,
        "GET",
        extracted["frames"][0]["url"].as_str().unwrap(),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
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
    remove_preview(&app, &extracted).await;
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
    remove_preview(&app, &extracted).await;
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
    remove_preview(&app, &extracted).await;
}

#[tokio::test]
async fn preview_rejects_encoded_absolute_and_parent_paths() {
    let dir = TempDir::new().unwrap();
    let app = app(&dir);
    let Some(extracted) = extracted_preview(&app, &dir).await else {
        return;
    };
    let outside = dir.path().join("outside.txt");
    std::fs::write(&outside, "synthetic outside-session fixture").unwrap();
    let absolute = outside.to_str().unwrap().replace('/', "%2F");
    for filename in [
        absolute.as_str(),
        "%2E%2E%2Foutside.txt",
        "..%5Coutside.txt",
    ] {
        let (status, _) = request(
            &app,
            "GET",
            &format!(
                "/api/preview/frames/{}/{filename}",
                extracted["preview_id"].as_str().unwrap()
            ),
            Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    remove_preview(&app, &extracted).await;
}

#[tokio::test]
async fn failed_extraction_removes_its_partial_session() {
    let dir = TempDir::new().unwrap();
    let app = app(&dir);
    let invalid = dir.path().join("invalid-video.mkv");
    std::fs::write(&invalid, "deliberately invalid media fixture").unwrap();
    let (status, _) = request(
        &app,
        "POST",
        "/api/preview/extract",
        json!({"video_path":invalid,"count":1}),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(std::fs::read_dir(dir.path().join("data/preview-cache"))
        .unwrap()
        .all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("session-")));
}

async fn wait_for_instance_lock_release(data_dir: &std::path::Path) {
    let lock_path = data_dir.join("instance.lock");
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let unlocked = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&lock_path)
                .is_ok_and(|file| file.try_lock().is_ok());
            if unlocked {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("previous AppState should release instance.lock");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn terminal_persistence_failure_holds_public_running_and_recovers_on_restart() {
    let dir = TempDir::new().unwrap();
    let data = dir.path().join("data");
    let config_path = dir.path().join("config.toml");
    let state = app_state_with_config(AppConfig::default(), config_path.clone(), data.clone());
    let db = rusqlite::Connection::open(data.join("jobs.db")).unwrap();
    db.execute_batch(
        "CREATE TRIGGER reject_terminal BEFORE UPDATE ON jobs \
         WHEN NEW.status = 'completed' \
         BEGIN SELECT RAISE(ABORT, 'injected terminal write failure'); END;",
    )
    .unwrap();

    let app = api_router(state.clone());
    let (status, created) = request(
        &app,
        "POST",
        "/api/jobs",
        json!({"workflow":{"nodes":[{"id":"a","node_type":"Constant","params":{}}],"connections":[]}}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["id"].as_str().unwrap().to_string();

    let sidecar = data.join("pending_terminal").join(format!("{id}.json"));
    tokio::time::timeout(Duration::from_secs(10), async {
        while !sidecar.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("sidecar should be written when terminal SQLite update fails");

    let (_, public_job) = request(&app, "GET", &format!("/api/jobs/{id}"), Value::Null).await;
    assert_eq!(
        public_job["status"], "running",
        "API must not expose completed while SQLite still holds running: {public_job}"
    );
    let db_status: String = db
        .query_row("SELECT status FROM jobs WHERE id = ?1", [&id], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(db_status, "running");

    // Simulate hard restart after transient DB fault clears.
    drop(app);
    drop(state);
    wait_for_instance_lock_release(&data).await;
    db.execute_batch("DROP TRIGGER reject_terminal;").unwrap();
    drop(db);

    let restarted = app_state_with_config(AppConfig::default(), config_path, data.clone());
    restarted.ensure_jobs_ready().unwrap();
    let restarted_app = api_router(restarted);
    let (http_status, restored_job) = request(
        &restarted_app,
        "GET",
        &format!("/api/jobs/{id}"),
        Value::Null,
    )
    .await;
    assert_eq!(http_status, StatusCode::OK);
    assert_eq!(restored_job["status"], "completed", "{restored_job}");
    assert!(!sidecar.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn startup_removes_staged_outputs_for_dynamically_computed_output_path() {
    let dir = TempDir::new().unwrap();
    let data = dir.path().join("data");
    let config_path = dir.path().join("config.toml");
    let source_media = dir.path().join("source.mkv");
    std::fs::write(&source_media, b"synthetic source fixture").unwrap();
    let computed_dir = dir.path().join("rendered").join("season-01");
    std::fs::create_dir_all(&computed_dir).unwrap();

    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let endpoint = Router::new().route(
        "/pause",
        get({
            let entered = entered.clone();
            let release = release.clone();
            move || {
                let entered = entered.clone();
                let release = release.clone();
                async move {
                    entered.notify_one();
                    release.notified().await;
                    "ok"
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let pause_url = format!("http://{}/pause", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, endpoint).await.unwrap();
    });

    // `VideoOutput.output_path` is computed dynamically by `StringTemplate` +
    // `PathJoiner` (neither a literal on `VideoOutput` nor a `WorkflowInput`),
    // and `hold` sits downstream of `sink` so `VideoOutput` is guaranteed to
    // have registered the path before the pause point is reached.
    let workflow = json!({
        "nodes": [
            {"id": "dir", "node_type": "Constant", "params": {"type": "Path", "value": computed_dir.to_string_lossy()}},
            {"id": "ep", "node_type": "Constant", "params": {"type": "Str", "value": "Episode 07"}},
            {"id": "name", "node_type": "StringTemplate", "params": {"template": "{str0}.mkv", "num_input": 1}},
            {"id": "join", "node_type": "PathJoiner", "params": {}},
            {"id": "src", "node_type": "Constant", "params": {"type": "Path", "value": source_media.to_string_lossy()}},
            {"id": "sink", "node_type": "VideoOutput", "params": {}},
            {"id": "divide", "node_type": "PathDivider", "params": {}},
            {"id": "hold", "node_type": "HttpRequest", "params": {"url": pause_url, "max_retries": 0}}
        ],
        "connections": [
            {"from_node": "ep", "from_port": "value", "to_node": "name", "to_port": "str0", "port_type": "Str"},
            {"from_node": "dir", "from_port": "value", "to_node": "join", "to_port": "parent_path", "port_type": "Path"},
            {"from_node": "name", "from_port": "value", "to_node": "join", "to_port": "file_name", "port_type": "Str"},
            {"from_node": "src", "from_port": "value", "to_node": "sink", "to_port": "source_path", "port_type": "Path"},
            {"from_node": "join", "from_port": "path", "to_node": "sink", "to_port": "output_path", "port_type": "Path"},
            {"from_node": "sink", "from_port": "output_path", "to_node": "divide", "to_port": "path", "port_type": "Path"},
            {"from_node": "divide", "from_port": "file_name", "to_node": "hold", "to_port": "body", "port_type": "Str"}
        ]
    });

    let state = app_state_with_config(AppConfig::default(), config_path.clone(), data.clone());
    let app = api_router(state.clone());
    let (status, created) = request(&app, "POST", "/api/jobs", json!({"workflow": workflow})).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let job_id = created["id"].as_str().unwrap().to_string();

    tokio::time::timeout(Duration::from_secs(10), entered.notified())
        .await
        .expect("job should execute VideoOutput and reach pause node while running");

    // Materialize a staged file owned by this running job beside the dynamically
    // computed output path, plus files that must NOT be removed.
    let staged_video = computed_dir.join(format!(".Episode 07.{job_id}.a1B2c3.videnoa-video.mkv"));
    let staged_mux = computed_dir.join(format!(".Episode 07.{job_id}.Zz9900.videnoa-mux.mkv"));
    let kept_video = computed_dir.join(format!(".Episode 07.{job_id}.K3pt00.videnoa-kept.mkv"));
    let other_job_staged = computed_dir.join(".Episode 07.other-job.a1B2c3.videnoa-video.mkv");
    for path in [&staged_video, &staged_mux, &kept_video, &other_job_staged] {
        std::fs::write(path, b"staged bytes").unwrap();
    }

    // Freeze `jobs.db` in `running` state to simulate a hard kill mid-encode.
    let db = rusqlite::Connection::open(data.join("jobs.db")).unwrap();
    db.execute_batch(
        "CREATE TRIGGER freeze_running BEFORE UPDATE ON jobs \
         BEGIN SELECT RAISE(ABORT, 'simulated hard crash'); END;",
    )
    .unwrap();
    release.notify_one();
    // Wait briefly for executor thread to exit before dropping `AppState`, then
    // remove any pending_terminal sidecar because in a real SIGKILL `finish_job`
    // never ran at all.
    let sidecar = data.join("pending_terminal").join(format!("{job_id}.json"));
    tokio::time::timeout(Duration::from_secs(5), async {
        while !sidecar.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    drop(app);
    drop(state);
    wait_for_instance_lock_release(&data).await;
    std::fs::remove_file(&sidecar).unwrap();
    db.execute_batch("DROP TRIGGER freeze_running;").unwrap();
    drop(db);
    server.abort();

    // Restart `AppState` on the same `data_dir`: the running job must become
    // `cancelled` AND its dynamically computed staged files must be deleted.
    let restarted = app_state_with_config(AppConfig::default(), config_path, data);
    restarted.ensure_jobs_ready().unwrap();
    let restarted_app = api_router(restarted);
    let (http_status, job_json) = request(
        &restarted_app,
        "GET",
        &format!("/api/jobs/{job_id}"),
        Value::Null,
    )
    .await;
    assert_eq!(http_status, StatusCode::OK);
    assert_eq!(job_json["status"], "cancelled", "{job_json}");
    assert!(!staged_video.exists(), "staged video file must be deleted");
    assert!(!staged_mux.exists(), "staged mux file must be deleted");
    assert!(kept_video.exists(), "kept encode must be preserved");
    assert!(
        other_job_staged.exists(),
        "another job's staged file must be preserved"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn workflow_overwrite_preserves_private_file_permissions_across_atomic_replace() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().unwrap();
    let app = app(&dir);

    let (status, created) = request(
        &app,
        "POST",
        "/api/workflows",
        json!({
            "name": "PrivateWorkflow",
            "description": "v1",
            "workflow": {"nodes": [], "connections": []}
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");

    let wf_path = dir
        .path()
        .join("data")
        .join("workflows")
        .join("PrivateWorkflow.json");
    std::fs::set_permissions(&wf_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let old_link = dir.path().join("private-v1.link");
    std::fs::hard_link(&wf_path, &old_link).unwrap();

    let (status, updated) = request(
        &app,
        "POST",
        "/api/workflows",
        json!({
            "name": "PrivateWorkflow",
            "description": "v2",
            "workflow": {"nodes": [], "connections": []},
            "overwrite": true
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{updated}");

    let mode = std::fs::metadata(&wf_path).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        mode, 0o600,
        "atomic workflow overwrite must preserve 0600 permissions, got {mode:04o}"
    );
    let new_doc: Value = serde_json::from_slice(&std::fs::read(&wf_path).unwrap()).unwrap();
    let old_doc: Value = serde_json::from_slice(&std::fs::read(&old_link).unwrap()).unwrap();
    assert_eq!(new_doc["description"], "v2");
    assert_eq!(old_doc["description"], "v1");
}
