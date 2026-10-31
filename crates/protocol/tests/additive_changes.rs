//! What a reader does with a message from a newer peer: unknown fields are
//! ignored and unknown enum values are readable.
use serde_json::json;
use videnoa_protocol::{
    CancelReason, ErrorCode, ErrorResponse, HealthResponse, HealthStatus, JobResponse, JobStatus,
    RunWorkflowRequest, PROTOCOL_VERSION,
};

fn job(status: &str) -> serde_json::Value {
    json!({
        "id": "6f1c2d3e-4b5a-4c6d-8e7f-001122334455",
        "status": status,
        "created_at": "2026-10-02T00:00:00Z",
        "started_at": null,
        "completed_at": null,
        "progress": null,
        "error": null,
        "workflow_name": "anime-2x",
        "workflow_source": "api_run_presets",
        "params": null,
        "rerun_of_job_id": null,
        "duration_ms": null,
    })
}

#[test]
fn unknown_enum_values_from_a_newer_worker_are_readable() {
    let health: HealthResponse =
        serde_json::from_value(json!({"status": "degraded", "protocol": 3})).expect("readable");
    assert_eq!(health.status, HealthStatus::Unknown);
    assert_eq!(health.protocol(), 3);

    let mut paused = job("paused");
    paused["cancel_requested"] = json!(true);
    paused["cancel_reason"] = json!("preempted");
    let paused: JobResponse = serde_json::from_value(paused).expect("readable");
    assert_eq!(paused.status, JobStatus::Unknown);
    assert_eq!(paused.cancel_reason, Some(CancelReason::Unknown));

    let error: ErrorResponse =
        serde_json::from_value(json!({"error": "later", "code": "gpu_on_fire"})).expect("readable");
    assert_eq!(error.code, Some(ErrorCode::Unknown));
}

#[test]
fn unknown_fields_are_ignored_on_responses_and_requests() {
    let mut extended = job("cancelled");
    extended["added_by_a_newer_worker"] = json!({"nested": [1, 2]});
    let job: JobResponse = serde_json::from_value(extended).expect("readable");
    assert_eq!(job.status, JobStatus::Cancelled);

    let request: RunWorkflowRequest = serde_json::from_value(json!({
        "workflow_name": "anime-2x",
        "added_by_a_newer_controller": true,
    }))
    .expect("an additive request field is accepted");
    assert_eq!(request.workflow_name.as_deref(), Some("anime-2x"));
}

#[test]
fn known_fields_are_still_required() {
    let mut missing = job("running");
    missing
        .as_object_mut()
        .expect("object")
        .remove("created_at");
    assert!(serde_json::from_value::<JobResponse>(missing).is_err());
    assert!(serde_json::from_value::<HealthResponse>(json!({})).is_err());
}

#[test]
fn current_health_appends_capacity_and_protocol() {
    // `status` stays first: scripts/tests/docker_slimming_contract_test.sh
    // matches the body against `^{"status":"ok"[,}]`.
    let health = HealthResponse {
        status: HealthStatus::Ok,
        gpu_slots: Some(1),
        running: Some(1),
        queued: Some(2),
        cancelling: Some(0),
        protocol: Some(PROTOCOL_VERSION),
    };
    assert_eq!(
        serde_json::to_string(&health).expect("serializes"),
        format!(
            r#"{{"status":"ok","gpu_slots":1,"running":1,"queued":2,"cancelling":0,"protocol":{PROTOCOL_VERSION}}}"#
        )
    );
}

#[test]
fn cancel_fields_appear_only_on_a_job_asked_to_stop() {
    let mut cancelled: JobResponse = serde_json::from_value(job("running")).expect("readable");
    let plain = serde_json::to_value(&cancelled).expect("serializes");
    assert!(plain.get("cancel_requested").is_none());

    cancelled.cancel_requested = true;
    cancelled.cancel_requested_at = Some(cancelled.created_at);
    cancelled.cancel_reason = Some(CancelReason::User);
    let asked = serde_json::to_value(&cancelled).expect("serializes");
    assert_eq!(asked["cancel_requested"], true);
    assert_eq!(asked["cancel_reason"], "user");
    assert_eq!(asked["cancel_requested_at"], "2026-10-02T00:00:00Z");
}
