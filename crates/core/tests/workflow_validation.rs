//! Graphs that `/api/jobs` accepts must be runnable, and graphs that cannot run
//! must be rejected at submission.
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    routing::post,
    Router,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
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

/// A "done" webhook fed by `VideoOutput.output_path` describes the finished
/// file, so it must fire only once that file has been published.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn webhook_on_the_output_path_fires_after_the_output_is_published() {
    let dir = TempDir::new().unwrap();
    let Some(input) = synthetic_video(dir.path()) else {
        eprintln!("skipping: ffmpeg is not available");
        return;
    };
    let output = dir.path().join("done.mp4");
    // Whether the posted path named an existing file when the webhook arrived.
    let deliveries = Arc::new(Mutex::new(Vec::new()));
    let endpoint = Router::new().route(
        "/done",
        post({
            let deliveries = deliveries.clone();
            move |body: String| {
                let deliveries = deliveries.clone();
                async move {
                    deliveries.lock().unwrap().push(Path::new(&body).is_file());
                    "ok"
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/done", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, endpoint).await.unwrap() });
    let app = app(&dir);
    let workflow = json!({
        "nodes": [
            {"id": "input", "node_type": "VideoInput", "params": {"path": input}},
            {"id": "output", "node_type": "VideoOutput", "params": {"output_path": output, "codec": "libx264", "pixel_format": "yuv420p"}},
            {"id": "to_str", "node_type": "TypeConversion", "params": {"input_type": "Path", "output_type": "Str"}},
            {"id": "done", "node_type": "HttpRequest", "params": {"method": "POST", "url": url, "max_retries": 0}}
        ],
        "connections": [
            {"from_node": "input", "from_port": "frames", "to_node": "output", "to_port": "frames", "port_type": "VideoFrames"},
            {"from_node": "input", "from_port": "source_path", "to_node": "output", "to_port": "source_path", "port_type": "Path"},
            {"from_node": "output", "from_port": "output_path", "to_node": "to_str", "to_port": "value", "port_type": "Path"},
            {"from_node": "to_str", "from_port": "value", "to_node": "done", "to_port": "body", "port_type": "Str"}
        ]
    });

    let (status, created) = request(&app, "POST", "/api/jobs", json!({"workflow": workflow})).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let job = terminal(&app, created["id"].as_str().unwrap()).await;
    server.abort();

    assert_eq!(job["status"], "completed", "{job:#}");
    assert_eq!(*deliveries.lock().unwrap(), vec![true], "{job:#}");
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

#[tokio::test]
async fn saved_workflow_records_its_schema_version() {
    let dir = TempDir::new().unwrap();
    let mut config = AppConfig::default();
    config.paths.workflows_dir = dir.path().join("workflows");
    let app = api_router(app_state_with_config(
        config,
        dir.path().join("config.toml"),
        dir.path().join("data"),
    ));
    let workflow =
        json!({"nodes": [{"id": "p", "node_type": "Print", "params": {}}], "connections": []});

    let (status, body) = request(
        &app,
        "POST",
        "/api/workflows",
        json!({"name": "versioned", "description": "", "workflow": workflow}),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    let saved: Value = serde_json::from_slice(
        &std::fs::read(dir.path().join("workflows").join("versioned.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(saved["workflow"]["schema_version"], json!(1), "{saved:#}");
}

/// Regression for Finding C: outer `Workflow` declares `interface_outputs: [declared]`,
/// while inner `WorkflowOutput` only defines `actual`. Validation must reject up front.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nested_interface_output_missing_from_inner_workflow_is_rejected() {
    let dir = TempDir::new().unwrap();
    let inner_path = dir.path().join("inner.json");
    let inner = json!({
        "nodes": [
            {"id": "constant", "node_type": "Constant", "params": {"type": "Str", "value": "synthetic-review-value"}},
            {"id": "out", "node_type": "WorkflowOutput", "params": {"ports": [{"name": "actual", "port_type": "Str", "required": true}]}}
        ],
        "connections": [
            {"from_node": "constant", "from_port": "value", "to_node": "out", "to_port": "actual", "port_type": "Str"}
        ]
    });
    std::fs::write(&inner_path, inner.to_string()).unwrap();

    let outer = json!({
        "nodes": [
            {"id": "nested", "node_type": "Workflow", "params": {
                "workflow_path": inner_path,
                "interface_outputs": [{"name": "declared", "port_type": "Str", "required": true}]
            }},
            {"id": "out", "node_type": "WorkflowOutput", "params": {"ports": [{"name": "result", "port_type": "Str", "required": true}]}}
        ],
        "connections": [
            {"from_node": "nested", "from_port": "declared", "to_node": "out", "to_port": "result", "port_type": "Str"}
        ]
    });

    let graph: videnoa_core::graph::PipelineGraph = serde_json::from_value(outer.clone()).unwrap();
    let registry = videnoa_core::registry::build_default_registry();
    let err = graph
        .validate(&registry)
        .expect_err("mismatched nested interface output must fail validate()");
    assert!(
        format!("{err:#}").contains("interface output 'declared' does not exist"),
        "{err:#}"
    );

    let app = app(&dir);
    let (status, body) = request(&app, "POST", "/api/jobs", json!({"workflow": outer})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let message = body.to_string();
    assert!(message.contains("node 'nested'"), "{message}");
    assert!(message.contains("declared"), "{message}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nested_interface_port_type_and_target_input_mismatches_are_rejected() {
    let dir = TempDir::new().unwrap();
    let inner_path = dir.path().join("typed_inner.json");
    let inner = json!({
        "nodes": [
            {"id": "wi", "node_type": "WorkflowInput", "params": {
                "ports": [{"name": "count", "port_type": "Int", "default_value": 1}]
            }},
            {"id": "to_str", "node_type": "TypeConversion", "params": {
                "input_type": "Int", "output_type": "Str"
            }},
            {"id": "wo", "node_type": "WorkflowOutput", "params": {
                "ports": [{"name": "label", "port_type": "Str"}]
            }}
        ],
        "connections": [
            {"from_node": "wi", "from_port": "count", "to_node": "to_str", "to_port": "value", "port_type": "Int"},
            {"from_node": "to_str", "from_port": "value", "to_node": "wo", "to_port": "label", "port_type": "Str"}
        ]
    });
    std::fs::write(&inner_path, inner.to_string()).unwrap();
    let app = app(&dir);

    // 1. Output PortType mismatch: inner produces Str, outer declares Int.
    let wrong_output_type = json!({
        "nodes": [
            {"id": "nested", "node_type": "Workflow", "params": {
                "workflow_path": inner_path,
                "interface_outputs": [{"name": "label", "port_type": "Int"}]
            }},
            {"id": "wo", "node_type": "WorkflowOutput", "params": {
                "ports": [{"name": "out", "port_type": "Int"}]
            }}
        ],
        "connections": [
            {"from_node": "nested", "from_port": "label", "to_node": "wo", "to_port": "out", "port_type": "Int"}
        ]
    });
    let (status, body) = request(
        &app,
        "POST",
        "/api/jobs",
        json!({"workflow": wrong_output_type}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body.to_string()
            .contains("interface output 'label' has type Int, expected Str"),
        "{body}"
    );

    // 2. Input PortType mismatch: inner expects Int, outer declares Str.
    let wrong_input_type = json!({
        "nodes": [
            {"id": "c", "node_type": "Constant", "params": {"type": "Str", "value": "seven"}},
            {"id": "nested", "node_type": "Workflow", "params": {
                "workflow_path": inner_path,
                "interface_inputs": [{"name": "count", "port_type": "Str"}],
                "interface_outputs": [{"name": "label", "port_type": "Str"}]
            }}
        ],
        "connections": [
            {"from_node": "c", "from_port": "value", "to_node": "nested", "to_port": "count", "port_type": "Str"}
        ]
    });
    let (status, body) = request(
        &app,
        "POST",
        "/api/jobs",
        json!({"workflow": wrong_input_type}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body.to_string()
            .contains("interface input 'count' has type Str, expected Int"),
        "{body}"
    );

    // 3. Non-existent target input declared on outer Workflow node.
    let ghost_input = json!({
        "nodes": [
            {"id": "c", "node_type": "Constant", "params": {"type": "Int", "value": 7}},
            {"id": "nested", "node_type": "Workflow", "params": {
                "workflow_path": inner_path,
                "interface_inputs": [{"name": "nonexistent", "port_type": "Int"}],
                "interface_outputs": [{"name": "label", "port_type": "Str"}]
            }}
        ],
        "connections": [
            {"from_node": "c", "from_port": "value", "to_node": "nested", "to_port": "nonexistent", "port_type": "Int"}
        ]
    });
    let (status, body) = request(&app, "POST", "/api/jobs", json!({"workflow": ghost_input})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body.to_string()
            .contains("interface input 'nonexistent' does not exist"),
        "{body}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nested_required_workflow_input_must_be_satisfied_at_submission() {
    let dir = TempDir::new().unwrap();
    let inner_path = dir.path().join("required_in.json");
    let inner = json!({
        "nodes": [
            {"id": "wi", "node_type": "WorkflowInput", "params": {
                "ports": [
                    {"name": "required_text", "port_type": "Str"},
                    {"name": "optional_suffix", "port_type": "Str", "default_value": "!"}
                ]
            }},
            {"id": "tpl", "node_type": "StringTemplate", "params": {
                "num_input": 2, "template": "{str0}{str1}", "strict": true
            }},
            {"id": "wo", "node_type": "WorkflowOutput", "params": {
                "ports": [{"name": "out", "port_type": "Str"}]
            }}
        ],
        "connections": [
            {"from_node": "wi", "from_port": "required_text", "to_node": "tpl", "to_port": "str0", "port_type": "Str"},
            {"from_node": "wi", "from_port": "optional_suffix", "to_node": "tpl", "to_port": "str1", "port_type": "Str"},
            {"from_node": "tpl", "from_port": "value", "to_node": "wo", "to_port": "out", "port_type": "Str"}
        ]
    });
    std::fs::write(&inner_path, inner.to_string()).unwrap();
    let app = app(&dir);

    // 1. Required inner input omitted from outer interface_inputs.
    let omitted = json!({
        "nodes": [{
            "id": "nested",
            "node_type": "Workflow",
            "params": {
                "workflow_path": inner_path,
                "interface_inputs": [],
                "interface_outputs": [{"name": "out", "port_type": "Str"}]
            }
        }],
        "connections": []
    });
    let (status, body) = request(&app, "POST", "/api/jobs", json!({"workflow": omitted})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body.to_string()
            .contains("missing required interface input 'required_text'"),
        "{body}"
    );

    // 2. Required inner input marked optional without default on outer Workflow node.
    let marked_optional = json!({
        "nodes": [{
            "id": "nested",
            "node_type": "Workflow",
            "params": {
                "workflow_path": inner_path,
                "interface_inputs": [{"name": "required_text", "port_type": "Str", "required": false}],
                "interface_outputs": [{"name": "out", "port_type": "Str"}]
            }
        }],
        "connections": []
    });
    let (status, body) = request(
        &app,
        "POST",
        "/api/jobs",
        json!({"workflow": marked_optional}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body.to_string()
            .contains("interface input 'required_text' is optional without a default"),
        "{body}"
    );

    // 3. Required inner input declared on outer Workflow node, but left unconnected without default/param.
    let unconnected = json!({
        "nodes": [{
            "id": "nested",
            "node_type": "Workflow",
            "params": {
                "workflow_path": inner_path,
                "interface_inputs": [{"name": "required_text", "port_type": "Str"}],
                "interface_outputs": [{"name": "out", "port_type": "Str"}]
            }
        }],
        "connections": []
    });
    let (status, body) = request(&app, "POST", "/api/jobs", json!({"workflow": unconnected})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body.to_string()
            .contains("node 'nested' missing required input port 'required_text'"),
        "{body}"
    );

    // 4. Positive control: outer default_value satisfies required inner input, while inner default_value
    // allows optional_suffix to be omitted from outer interface_inputs.
    let satisfied = json!({
        "nodes": [
            {
                "id": "nested",
                "node_type": "Workflow",
                "params": {
                    "workflow_path": inner_path,
                    "interface_inputs": [{
                        "name": "required_text",
                        "port_type": "Str",
                        "required": false,
                        "default_value": "hello"
                    }],
                    "interface_outputs": [{"name": "out", "port_type": "Str"}]
                }
            },
            {
                "id": "wo",
                "node_type": "WorkflowOutput",
                "params": {"ports": [{"name": "final", "port_type": "Str"}]}
            }
        ],
        "connections": [
            {"from_node": "nested", "from_port": "out", "to_node": "wo", "to_port": "final", "port_type": "Str"}
        ]
    });
    let (status, created) =
        request(&app, "POST", "/api/jobs", json!({"workflow": satisfied})).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let job = terminal(&app, created["id"].as_str().unwrap()).await;
    assert_eq!(job["status"], "completed", "{job:#}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn multi_level_nested_interface_mismatch_is_caught_at_outer_validate() {
    let dir = TempDir::new().unwrap();
    let path_c = dir.path().join("level_c.json");
    let path_b = dir.path().join("level_b.json");

    // Level C: valid standalone workflow producing 'actual_c' (Str) from 'x' (Int).
    let workflow_c = json!({
        "nodes": [
            {"id": "wi", "node_type": "WorkflowInput", "params": {
                "ports": [{"name": "x", "port_type": "Int"}]
            }},
            {"id": "conv", "node_type": "TypeConversion", "params": {
                "input_type": "Int", "output_type": "Str"
            }},
            {"id": "wo", "node_type": "WorkflowOutput", "params": {
                "ports": [{"name": "actual_c", "port_type": "Str"}]
            }}
        ],
        "connections": [
            {"from_node": "wi", "from_port": "x", "to_node": "conv", "to_port": "value", "port_type": "Int"},
            {"from_node": "conv", "from_port": "value", "to_node": "wo", "to_port": "actual_c", "port_type": "Str"}
        ]
    });
    std::fs::write(&path_c, workflow_c.to_string()).unwrap();

    // Level B: agrees with outer A on (x: Int -> y: Str), but misdeclares C's output as 'wrong_c'.
    let workflow_b = json!({
        "nodes": [
            {"id": "wi", "node_type": "WorkflowInput", "params": {
                "ports": [{"name": "x", "port_type": "Int"}]
            }},
            {"id": "call_c", "node_type": "Workflow", "params": {
                "workflow_path": path_c,
                "interface_inputs": [{"name": "x", "port_type": "Int"}],
                "interface_outputs": [{"name": "wrong_c", "port_type": "Str"}]
            }},
            {"id": "wo", "node_type": "WorkflowOutput", "params": {
                "ports": [{"name": "y", "port_type": "Str"}]
            }}
        ],
        "connections": [
            {"from_node": "wi", "from_port": "x", "to_node": "call_c", "to_port": "x", "port_type": "Int"},
            {"from_node": "call_c", "from_port": "wrong_c", "to_node": "wo", "to_port": "y", "port_type": "Str"}
        ]
    });
    std::fs::write(&path_b, workflow_b.to_string()).unwrap();

    // Level A: outer workflow calling B with matching (x: Int -> y: Str).
    let workflow_a = json!({
        "nodes": [
            {"id": "c", "node_type": "Constant", "params": {"type": "Int", "value": 42}},
            {"id": "call_b", "node_type": "Workflow", "params": {
                "workflow_path": path_b,
                "interface_inputs": [{"name": "x", "port_type": "Int"}],
                "interface_outputs": [{"name": "y", "port_type": "Str"}]
            }},
            {"id": "wo", "node_type": "WorkflowOutput", "params": {
                "ports": [{"name": "result", "port_type": "Str"}]
            }}
        ],
        "connections": [
            {"from_node": "c", "from_port": "value", "to_node": "call_b", "to_port": "x", "port_type": "Int"},
            {"from_node": "call_b", "from_port": "y", "to_node": "wo", "to_port": "result", "port_type": "Str"}
        ]
    });

    let graph: videnoa_core::graph::PipelineGraph =
        serde_json::from_value(workflow_a.clone()).unwrap();
    let registry = videnoa_core::registry::build_default_registry();
    let err = graph
        .validate(&registry)
        .expect_err("A -> B -> C deep mismatch must be caught at A.validate()");
    let full = format!("{err:#}");
    assert!(full.contains("in node 'call_c'"), "{full}");
    assert!(
        full.contains("interface output 'wrong_c' does not exist"),
        "{full}"
    );

    let app = app(&dir);
    let (status, body) = request(&app, "POST", "/api/jobs", json!({"workflow": workflow_a})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.to_string().contains("wrong_c"), "{body}");
}

/// When outer `Workflow` declares `interface_inputs: [{"name": "x", "port_type": "Int"}]`
/// without copying `default_value` and without an explicit `"required"` key (as the
/// web editor exports for injected `WorkflowInput` params), leaving `x` unconnected
/// must succeed whenever inner `WorkflowInput` provides either a `default_value`
/// or an injected node param.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unconnected_interface_input_with_inner_default_or_injected_param_runs() {
    let dir = TempDir::new().unwrap();
    let app = app(&dir);

    let inner_with_default = dir.path().join("inner_default.json");
    std::fs::write(
        &inner_with_default,
        json!({
            "nodes": [
                {"id": "wi", "node_type": "WorkflowInput", "params": {
                    "ports": [{"name": "x", "port_type": "Int", "default_value": 10}]
                }},
                {"id": "wo", "node_type": "WorkflowOutput", "params": {
                    "ports": [{"name": "x", "port_type": "Int"}]
                }}
            ],
            "connections": [
                {"from_node": "wi", "from_port": "x", "to_node": "wo", "to_port": "x", "port_type": "Int"}
            ]
        })
        .to_string(),
    )
    .unwrap();

    let inner_with_injected = dir.path().join("inner_injected.json");
    std::fs::write(
        &inner_with_injected,
        json!({
            "nodes": [
                {"id": "wi", "node_type": "WorkflowInput", "params": {
                    "ports": [{"name": "x", "port_type": "Int"}],
                    "x": 10
                }},
                {"id": "wo", "node_type": "WorkflowOutput", "params": {
                    "ports": [{"name": "x", "port_type": "Int"}]
                }}
            ],
            "connections": [
                {"from_node": "wi", "from_port": "x", "to_node": "wo", "to_port": "x", "port_type": "Int"}
            ]
        })
        .to_string(),
    )
    .unwrap();

    for inner_path in [&inner_with_default, &inner_with_injected] {
        let outer = json!({
            "nodes": [
                {
                    "id": "nested",
                    "node_type": "Workflow",
                    "params": {
                        "workflow_path": inner_path,
                        "interface_inputs": [{"name": "x", "port_type": "Int"}],
                        "interface_outputs": [{"name": "x", "port_type": "Int"}]
                    }
                },
                {
                    "id": "wo",
                    "node_type": "WorkflowOutput",
                    "params": {"ports": [{"name": "result", "port_type": "Int"}]}
                }
            ],
            "connections": [
                {"from_node": "nested", "from_port": "x", "to_node": "wo", "to_port": "result", "port_type": "Int"}
            ]
        });

        let (status, created) =
            request(&app, "POST", "/api/jobs", json!({"workflow": outer})).await;
        assert_eq!(status, StatusCode::CREATED, "{created}");
        let job = terminal(&app, created["id"].as_str().unwrap()).await;
        assert_eq!(job["status"], "completed", "{job:#}");
    }
}
