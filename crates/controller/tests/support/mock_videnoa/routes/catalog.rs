use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde_json::{json, Value};

use super::{body_bytes, error_response, journal_request, json_response, record, MAX_JSON_BYTES};
use crate::mock_videnoa::domain::{PresetResponse, WorkflowEntry, WorkflowInterface, WorkflowPort};
use crate::mock_videnoa::journal::{JournalOutcome, Route};
use crate::mock_videnoa::state::SharedState;

pub(crate) async fn health(State(state): State<Arc<SharedState>>, request: Request) -> Response {
    simple(state, request, Route::Health, json!({"status": "ok"})).await
}

pub(crate) async fn workflows(State(state): State<Arc<SharedState>>, request: Request) -> Response {
    simple(state, request, Route::Workflows, saved_workflows()).await
}

pub(crate) async fn presets(State(state): State<Arc<SharedState>>, request: Request) -> Response {
    simple(state, request, Route::Presets, preset_entries()).await
}

pub(crate) async fn workflow_interface(
    State(state): State<Arc<SharedState>>,
    Path(filename): Path<String>,
    request: Request,
) -> Response {
    let Ok((parts, body)) = body_bytes(request, MAX_JSON_BYTES).await else {
        return error_response(StatusCode::BAD_REQUEST, "invalid_body");
    };
    let sequence = state.inner.lock().await.begin(Route::Interface);
    let journal = journal_request(&parts, &body, Route::Interface, sequence, BTreeMap::new());
    let interface = match filename.as_str() {
        "eligible-workflow.json" | "eligible-preset" => Some(eligible_interface()),
        "missing-path.json" => Some(missing_path_interface()),
        "wrong-path-type.json" => Some(wrong_path_interface()),
        _ => None,
    };
    let response = match interface {
        Some(interface) => json_response(StatusCode::OK, &interface),
        None => error_response(StatusCode::NOT_FOUND, "not_found"),
    };
    record(
        &state,
        journal,
        response.status(),
        JournalOutcome::Delivered,
    )
    .await;
    response
}

async fn simple<T: serde::Serialize>(
    state: Arc<SharedState>,
    request: Request,
    route: Route,
    value: T,
) -> Response {
    let Ok((parts, body)) = body_bytes(request, MAX_JSON_BYTES).await else {
        return error_response(StatusCode::BAD_REQUEST, "invalid_body");
    };
    let sequence = state.inner.lock().await.begin(route);
    let journal = journal_request(&parts, &body, route, sequence, BTreeMap::new());
    let response = json_response(StatusCode::OK, &value);
    record(&state, journal, StatusCode::OK, JournalOutcome::Delivered).await;
    response
}

pub(crate) fn saved_workflows() -> Vec<WorkflowEntry> {
    vec![
        workflow_entry(
            "eligible-workflow.json",
            "Eligible Workflow",
            &eligible_interface(),
        ),
        workflow_entry(
            "missing-path.json",
            "Missing Path",
            &missing_path_interface(),
        ),
        workflow_entry(
            "wrong-path-type.json",
            "Wrong Path Type",
            &wrong_path_interface(),
        ),
    ]
}

pub(crate) fn preset_entries() -> Vec<PresetResponse> {
    vec![PresetResponse {
        id: "eligible-preset".to_owned(),
        name: "Eligible Preset".to_owned(),
        description: "Test-only compatible preset".to_owned(),
        workflow: workflow_document(&eligible_interface()),
    }]
}

fn workflow_entry(filename: &str, name: &str, interface: &WorkflowInterface) -> WorkflowEntry {
    WorkflowEntry {
        filename: filename.to_owned(),
        name: name.to_owned(),
        description: format!("Test-only {name}"),
        workflow: workflow_document(interface),
        has_interface: true,
    }
}

fn workflow_document(interface: &WorkflowInterface) -> Value {
    json!({"nodes": [], "connections": [], "interface": interface})
}

fn port(name: &str, port_type: &str) -> WorkflowPort {
    WorkflowPort {
        name: name.to_owned(),
        port_type: port_type.to_owned(),
        default_value: None,
    }
}

fn eligible_interface() -> WorkflowInterface {
    WorkflowInterface {
        inputs: vec![port("input", "Path"), port("output", "Path")],
        outputs: Vec::new(),
    }
}

fn missing_path_interface() -> WorkflowInterface {
    WorkflowInterface {
        inputs: vec![port("input", "Path")],
        outputs: Vec::new(),
    }
}

fn wrong_path_interface() -> WorkflowInterface {
    WorkflowInterface {
        inputs: vec![port("input", "String"), port("output", "Path")],
        outputs: Vec::new(),
    }
}
