//! The wire types read what v0.1.10 Workers and Controllers send and write
//! the same bytes for every field v0.1.10 knows.
//!
//! The fixtures under `fixtures/v0.1.10/` are serde's output for the v0.1.10
//! response and request structs (`crates/core/src/server/mod.rs` and
//! `files.rs`, `crates/controller/src/remote/dto.rs` at tag v0.1.10): fields
//! in declaration order, compact, one document per file. Free-form JSON
//! (`workflow`, `params`) is written with sorted keys, as `serde_json::Value`
//! writes it.
use serde::de::DeserializeOwned;
use serde::Serialize;
use videnoa_protocol::{
    CreateJobResponse, ErrorCode, ErrorResponse, FileStatResponse, HealthResponse, HealthStatus,
    JobResponse, JobStatus, PresetResponse, RunWorkflowRequest, UploadResponse, WorkflowEntry,
    WorkflowInterface, LEGACY_PROTOCOL_VERSION,
};

fn fixture(name: &str) -> String {
    let path = format!(
        "{}/tests/fixtures/v0.1.10/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{path}: {error}"))
        .trim_end()
        .to_owned()
}

/// Reads a fixture and checks that writing it back gives the same bytes.
fn round_trip<T: DeserializeOwned + Serialize>(name: &str) -> T {
    let json = fixture(name);
    let value: T = serde_json::from_str(&json).unwrap_or_else(|error| panic!("{name}: {error}"));
    assert_eq!(
        serde_json::to_string(&value).expect("serializes"),
        json,
        "{name} is written differently than v0.1.10 wrote it"
    );
    value
}

#[test]
fn health_of_a_v0_1_10_worker_reads_as_protocol_1() {
    let health: HealthResponse = round_trip("health.json");
    assert_eq!(health.status, HealthStatus::Ok);
    assert_eq!(health.protocol(), LEGACY_PROTOCOL_VERSION);
    assert_eq!(health.gpu_slots, None);
}

#[test]
fn jobs_and_receipts_keep_their_v0_1_10_form() {
    let running: JobResponse = round_trip("job-running.json");
    assert_eq!(running.status, JobStatus::Running);
    assert!(!running.cancel_requested);
    assert_eq!(running.cancel_reason, None);
    assert_eq!(
        running
            .params
            .as_ref()
            .and_then(|params| params.get("input")),
        Some(&serde_json::json!("6f1c/input.mkv"))
    );
    let failed: JobResponse = round_trip("job-failed.json");
    assert_eq!(failed.status, JobStatus::Failed);
    assert_eq!(failed.duration_ms, Some(60_500));
    let receipt: CreateJobResponse = round_trip("run-receipt.json");
    assert_eq!(receipt.status, JobStatus::Queued);
}

#[test]
fn errors_keep_their_v0_1_10_form() {
    let plain: ErrorResponse = round_trip("error.json");
    assert_eq!(plain.code, None);
    let coded: ErrorResponse = round_trip("error-coded.json");
    assert_eq!(coded.code, Some(ErrorCode::IdempotencyConflict));
}

#[test]
fn catalog_and_file_responses_keep_their_v0_1_10_form() {
    let workflows: Vec<WorkflowEntry> = round_trip("workflows.json");
    assert!(workflows[0].has_interface);
    let presets: Vec<PresetResponse> = round_trip("presets.json");
    assert_eq!(presets[0].id, "anime-2x");
    let interface: WorkflowInterface = round_trip("interface.json");
    assert_eq!(
        interface.inputs[1].default_value,
        Some(serde_json::json!(2))
    );
    let upload: UploadResponse = round_trip("upload.json");
    assert_eq!(upload.size, 3);
    let stat: FileStatResponse = round_trip("stat.json");
    assert!(stat.is_dir);
}

#[test]
fn run_requests_keep_the_v0_1_10_controller_form() {
    let request: RunWorkflowRequest = round_trip("run-request.json");
    assert_eq!(request.workflow_name.as_deref(), Some("anime-2x"));
    // The validation request carries only the name.
    let validate = RunWorkflowRequest {
        workflow_name: Some("anime-2x".to_owned()),
        params: None,
    };
    assert_eq!(
        serde_json::to_string(&validate).expect("serializes"),
        r#"{"workflow_name":"anime-2x"}"#
    );
}
