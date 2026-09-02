use std::error::Error;
use std::fs;

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};
use videnoa_controller::domain::{
    ApiErrorEnvelope, CancelTaskResponse, HealthResponse, IdempotencyKey, LoginRequest,
    LoginResponse, LogoutResponse, ReadinessResponse, RetryTaskResponse, SessionResponse,
    SettingsResponse, SettingsUpdateRequest, SseEvent, TaskActionRequest, TaskCreateRequest,
    TaskDetailResponse, TaskListQuery, TaskListResponse, WorkerCreateRequest, WorkerDeleteResponse,
    WorkerListResponse, WorkerUpdateRequest,
};

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

fn roundtrip<T>(value: Value) -> TestResult<Value>
where
    T: DeserializeOwned + Serialize,
{
    Ok(serde_json::to_value(serde_json::from_value::<T>(value)?)?)
}

fn progress() -> Value {
    json!({
        "percent": 37.5,
        "processed_frames": 900,
        "total_frames": 2400,
        "frames_per_second": 12.5,
        "eta_seconds": 120,
        "bytes_transferred": 1_048_576,
        "bytes_total": 4_194_304
    })
}

fn task() -> Value {
    json!({
        "id": "00000000-0000-4000-8000-000000000001",
        "status": "processing",
        "input_path": "/nas/input/Season ../episode.v1.mkv",
        "output_path": "/nas/output/Season ../episode.final.mp4",
        "input_extension": "mkv",
        "output_extension": "mp4",
        "workflow": "anime upscale ../v2",
        "priority": 17,
        "source": "api",
        "source_reference": "ani-rss:item/0042",
        "worker_id": "00000000-0000-4000-8000-000000000003",
        "progress": progress(),
        "attempt_count": 1,
        "failure": null,
        "cancel_requested_at": null,
        "created_at": "2026-09-02T00:00:00Z",
        "updated_at": "2026-09-02T00:05:00Z",
        "completed_at": null
    })
}

fn attempt() -> Value {
    json!({
        "id": "00000000-0000-4000-8000-000000000002",
        "task_id": "00000000-0000-4000-8000-000000000001",
        "attempt_number": 1,
        "worker_id": "00000000-0000-4000-8000-000000000003",
        "status": "processing",
        "submission_key": "00000000-0000-4000-8000-000000000004",
        "remote_job_id": "00000000-0000-4000-8000-000000000005",
        "remote_input_path": "task/input/../opaque.mkv",
        "remote_output_path": "task/output/../opaque.mp4",
        "progress": progress(),
        "retry": {"retry_count": 0, "next_retry_at": null},
        "failure": null,
        "created_at": "2026-09-02T00:00:01Z",
        "started_at": "2026-09-02T00:00:02Z",
        "completed_at": null
    })
}

fn worker() -> Value {
    json!({
        "id": "00000000-0000-4000-8000-000000000003",
        "version": 7,
        "name": "videnoa-east",
        "api_url": "https://worker.example/api/",
        "enabled": true,
        "online": true,
        "compute_slots": 2,
        "capabilities": {
            "workflows": [{"name": "anime upscale ../v2", "kind": "workflow"}],
            "refreshed_at": "2026-09-02T00:04:00Z"
        },
        "capacity": {
            "used_slots": 1,
            "available_slots": 1,
            "assigned_tasks": 2,
            "staged_tasks": 1,
            "processing_tasks": 1,
            "active_uploads": 1,
            "active_downloads": 0,
            "progress": progress()
        },
        "last_seen_at": "2026-09-02T00:05:00Z",
        "last_assigned_at": "2026-09-02T00:00:01Z",
        "created_at": "2026-09-01T00:00:00Z",
        "updated_at": "2026-09-02T00:05:00Z",
        "last_error": null
    })
}

fn settings() -> Value {
    json!({
        "version": 3,
        "paths": {
            "input_roots": ["/nas/input"],
            "output_roots": ["/nas/output"],
            "data_root": "/var/lib/videnoa-controller",
            "temp_root": "/var/lib/videnoa-controller/temp",
            "password_hash_file": "/run/secrets/admin-password.phc"
        },
        "secure_cookie": true,
        "session_absolute_seconds": 86400,
        "session_idle_seconds": 3600,
        "scheduler": {
            "paused": false,
            "default_compute_slots": 1,
            "prefetch_per_worker": 1,
            "max_concurrent_uploads": 1,
            "max_concurrent_downloads": 1
        },
        "timeouts": {"health_seconds": 10, "poll_seconds": 5, "transfer_seconds": 300},
        "retry": {"initial_seconds": 1, "maximum_seconds": 60, "max_attempts": 5}
    })
}

#[test]
fn all_public_http_contracts_roundtrip_and_can_write_evidence() -> TestResult {
    // Given: deterministic values spanning every Task 2 public HTTP DTO family.
    let task_value = task();
    let attempt = attempt();
    let worker_value = worker();
    let settings_value = settings();
    let session = json!({
        "id": "00000000-0000-4000-8000-000000000006",
        "authenticated": true,
        "method": "session",
        "expires_at": "2026-09-03T00:00:00Z",
        "idle_expires_at": "2026-09-02T01:00:00Z"
    });

    // When: each schema crosses its typed serde boundary.
    let contracts = json!({
        "task_create": roundtrip::<TaskCreateRequest>(json!({
            "input_path": "/nas/input/Season ../episode.v1.mkv",
            "output_path": "/nas/output/Season ../episode.final.mp4",
            "workflow": "anime upscale ../v2",
            "priority": 17,
            "source": "api",
            "source_reference": "ani-rss:item/0042"
        }))?,
        "task_idempotency_key": roundtrip::<IdempotencyKey>(json!("task-ingress-0001"))?,
        "task_detail": roundtrip::<TaskDetailResponse>(json!({"task": task_value, "attempts": [attempt]}))?,
        "task_list_query": roundtrip::<TaskListQuery>(json!({}))?,
        "task_list": roundtrip::<TaskListResponse>(json!({
            "items": [task()], "total": 1, "limit": 100, "offset": 0
        }))?,
        "task_action": roundtrip::<TaskActionRequest>(json!({"version": 4}))?,
        "cancel": roundtrip::<CancelTaskResponse>(json!({
            "task_id": "00000000-0000-4000-8000-000000000001",
            "status": "processing",
            "cancel_requested_at": "2026-09-02T00:06:00Z"
        }))?,
        "retry": roundtrip::<RetryTaskResponse>(json!({
            "task_id": "00000000-0000-4000-8000-000000000001",
            "attempt_id": "00000000-0000-4000-8000-000000000007",
            "status": "queued"
        }))?,
        "worker_create": roundtrip::<WorkerCreateRequest>(json!({
            "name": "videnoa-east", "api_url": "https://worker.example/api/",
            "enabled": true, "compute_slots": 2
        }))?,
        "worker_update": roundtrip::<WorkerUpdateRequest>(json!({
            "version": 7, "name": "videnoa-east", "api_url": "https://worker.example/api/",
            "enabled": true, "compute_slots": 2
        }))?,
        "worker_delete": roundtrip::<WorkerDeleteResponse>(json!({
            "worker_id": "00000000-0000-4000-8000-000000000003", "deleted": true
        }))?,
        "worker_list": roundtrip::<WorkerListResponse>(json!({"items": [worker_value], "total": 1}))?,
        "settings": roundtrip::<SettingsResponse>(settings_value.clone())?,
        "settings_update": roundtrip::<SettingsUpdateRequest>(json!({
            "version": 3,
            "scheduler": settings_value["scheduler"],
            "timeouts": settings_value["timeouts"],
            "retry": settings_value["retry"]
        }))?,
        "login_request_fields": ["password"],
        "login": roundtrip::<LoginResponse>(json!({"session": session.clone()}))?,
        "session": roundtrip::<SessionResponse>(session.clone())?,
        "logout": roundtrip::<LogoutResponse>(json!({"logged_out": true}))?,
        "health": roundtrip::<HealthResponse>(json!({"status": "ok"}))?,
        "readiness": roundtrip::<ReadinessResponse>(json!({
            "status": "ready", "checks": [{"name": "configuration", "ready": true, "message": null}]
        }))?,
        "api_error": roundtrip::<ApiErrorEnvelope>(json!({"error": {
            "code": "invalid_request", "message": "request validation failed", "retryable": false,
            "field_errors": [{"field": "limit", "code": "out_of_range", "message": "maximum is 500"}]
        }}))?,
        "sse_task": roundtrip::<SseEvent>(json!({
            "type": "task_updated", "data": {"event_id": "00000000-0000-4000-8000-000000000008", "task": task()}
        }))?,
        "sse_worker": roundtrip::<SseEvent>(json!({
            "type": "worker_updated", "data": {"event_id": "00000000-0000-4000-8000-000000000009", "worker": worker()}
        }))?,
        "sse_scheduler": roundtrip::<SseEvent>(json!({
            "type": "scheduler_updated", "data": {
                "event_id": "00000000-0000-4000-8000-000000000010", "scheduler": settings_value["scheduler"]
            }
        }))?
    });
    let _: LoginRequest = serde_json::from_value(json!({"password": "test-only-value"}))?;
    let output = format!("{}\n", serde_json::to_string_pretty(&contracts)?);

    // Then: output is stable, health stays Task 1-compatible, and no credential value is emitted.
    assert_eq!(contracts["health"], json!({"status": "ok"}));
    assert!(!output.contains("test-only-value"));
    if let Some(path) = std::env::var_os("VIDENOA_CONTRACT_EVIDENCE") {
        fs::write(path, output)?;
    }
    Ok(())
}
