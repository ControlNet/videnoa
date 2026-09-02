mod api_basic;
mod api_concurrency;
mod persistence;
mod restart;
mod workflow_independence;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::Router;
use dashmap::DashMap;
use serde_json::Value;
use tempfile::TempDir;
use tower::ServiceExt;

use super::{app_router, AppState, JobStatus, Preset};
use crate::config::AppConfig;
use crate::model_registry::ModelRegistry;
use crate::node::{ExecutionContext, Node};
use crate::registry::NodeRegistry;
use crate::types::PortData;

const WORKFLOW_NAME: &str = "idempotent-run";
const IDEMPOTENCY_HEADER: &str = "idempotency-key";

struct RunFixture {
    _root: TempDir,
    state: AppState,
    data_dir: PathBuf,
    workflows_dir: PathBuf,
    presets_dir: PathBuf,
}

impl RunFixture {
    fn new(delay_ms: u64) -> Result<Self> {
        let root = TempDir::new()?;
        let data_dir = root.path().join("data");
        let workflows_dir = root.path().join("workflows");
        let presets_dir = root.path().join("presets");
        std::fs::create_dir_all(&workflows_dir)?;
        std::fs::create_dir_all(&presets_dir)?;
        write_workflow(&workflows_dir, delay_ms)?;
        let state = build_state(root.path(), &data_dir, &workflows_dir, &presets_dir);
        Ok(Self {
            _root: root,
            state,
            data_dir,
            workflows_dir,
            presets_dir,
        })
    }

    fn router(&self) -> Router {
        app_router(self.state.clone())
    }

    fn restarted_state(&self) -> AppState {
        build_state(
            self._root.path(),
            &self.data_dir,
            &self.workflows_dir,
            &self.presets_dir,
        )
    }
}

fn build_state(root: &Path, data_dir: &Path, workflows_dir: &Path, presets_dir: &Path) -> AppState {
    let mut node_registry = NodeRegistry::new();
    node_registry.register("idempotency_delay", |params| {
        let delay_ms = params
            .get("delay_ms")
            .and_then(Value::as_u64)
            .unwrap_or_default();
        Ok(Box::new(DelayNode { delay_ms }))
    });
    let mut config = AppConfig::default();
    config.paths.workflows_dir = workflows_dir.to_path_buf();
    config.paths.presets_dir = presets_dir.to_path_buf();
    config.paths.models_dir = root.join("models");
    AppState::new(
        node_registry,
        ModelRegistry::with_builtin_models(config.paths.models_dir.clone()),
        DashMap::<String, Preset>::new(),
        config,
        root.join("config.toml"),
        data_dir.to_path_buf(),
    )
}

fn write_workflow(directory: &Path, delay_ms: u64) -> Result<()> {
    let document = serde_json::json!({
        "workflow": {
            "nodes": [{
                "id": "delay",
                "node_type": "idempotency_delay",
                "params": {"delay_ms": delay_ms}
            }],
            "connections": []
        }
    });
    std::fs::write(
        directory.join(format!("{WORKFLOW_NAME}.json")),
        serde_json::to_vec(&document)?,
    )?;
    Ok(())
}

fn request(key: Option<&str>, params: Value) -> Request<Body> {
    let body = serde_json::json!({"workflow_name": WORKFLOW_NAME, "params": params});
    raw_request(
        key,
        serde_json::to_vec(&body).expect("serialize request body"),
    )
}

fn raw_request(key: Option<&str>, body: Vec<u8>) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/api/run")
        .header("content-type", "application/json");
    if let Some(key) = key {
        builder = builder.header(IDEMPOTENCY_HEADER, key);
    }
    builder.body(Body::from(body)).expect("build request")
}

async fn response_json(router: Router, request: Request<Body>) -> Result<(StatusCode, Value)> {
    let response = router.oneshot(request).await?;
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024 * 1024).await?;
    Ok((status, serde_json::from_slice(&body)?))
}

fn persisted_job_count(data_dir: &Path) -> Result<u64> {
    let connection = rusqlite::Connection::open(data_dir.join("jobs.db"))?;
    Ok(connection.query_row("SELECT COUNT(*) FROM jobs", [], |row| row.get(0))?)
}

fn persisted_job_id(data_dir: &Path) -> Result<String> {
    let connection = rusqlite::Connection::open(data_dir.join("jobs.db"))?;
    Ok(connection.query_row("SELECT id FROM jobs", [], |row| row.get(0))?)
}

struct DelayNode {
    delay_ms: u64,
}

impl Node for DelayNode {
    fn node_type(&self) -> &str {
        "idempotency_delay"
    }

    fn input_ports(&self) -> Vec<crate::node::PortDefinition> {
        Vec::new()
    }

    fn output_ports(&self) -> Vec<crate::node::PortDefinition> {
        Vec::new()
    }

    fn execute(
        &mut self,
        _inputs: &HashMap<String, PortData>,
        _context: &ExecutionContext,
    ) -> Result<HashMap<String, PortData>> {
        std::thread::sleep(std::time::Duration::from_millis(self.delay_ms));
        Ok(HashMap::new())
    }
}
