use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{header::HeaderName, HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use serde_json::Value;

use super::super::catalog::{preset_entries, saved_workflows};
use super::super::{
    body_bytes, error_response, journal_request, json_response, raw_json_response, record,
    DROP_RESPONSE_HEADER, MAX_JSON_BYTES,
};
use crate::mock_videnoa::checkpoints::Checkpoint;
use crate::mock_videnoa::domain::{
    CreateJobResponse, JobRecord, JobResponse, JobStatus, RunRequest,
};
use crate::mock_videnoa::fingerprint::run_fingerprint;
use crate::mock_videnoa::journal::{JournalOutcome, Route};
use crate::mock_videnoa::persistence::IdempotencyRecord;
use crate::mock_videnoa::state::{HarnessError, RuntimeState, SharedState};

struct PreparedRun {
    key: Option<String>,
    workflow_name: String,
    workflow_source: &'static str,
    params: Option<BTreeMap<String, Value>>,
    fingerprint: String,
}

struct PersistedRun {
    sequence: u64,
    status: StatusCode,
    creation: Option<CreateJobResponse>,
    drop_response: bool,
}

struct RunParseError {
    status: StatusCode,
    code: &'static str,
}

pub(crate) async fn run(State(state): State<Arc<SharedState>>, request: Request) -> Response {
    let Ok((parts, body)) = body_bytes(request, MAX_JSON_BYTES).await else {
        return error_response(StatusCode::BAD_REQUEST, "invalid_body");
    };
    let prepared = match prepare_run(&parts.headers, &body) {
        Ok(prepared) => prepared,
        Err(error) => return error_response(error.status, error.code),
    };
    let mut checkpoints = BTreeMap::new();
    state
        .checkpoint(Checkpoint::BeforeRunPersistence, &mut checkpoints)
        .await;
    if let Some(fault) = state.take_response_fault(Route::Run).await {
        let status = match StatusCode::from_u16(fault.status) {
            Ok(status) => status,
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let sequence = state.inner.lock().await.begin(Route::Run);
        let journal = journal_request(&parts, &body, Route::Run, sequence, checkpoints);
        record(&state, journal, status, JournalOutcome::FaultStatus).await;
        return raw_json_response(status, fault.body);
    }
    let Ok(persisted) = persist_run(&state, &prepared).await else {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, "persistence_failed");
    };
    if persisted.status == StatusCode::CREATED {
        state
            .checkpoint(
                Checkpoint::AfterRunPersistedBeforeResponse,
                &mut checkpoints,
            )
            .await;
    }
    let outcome = if persisted.drop_response {
        JournalOutcome::TransportDropped
    } else if persisted.status.is_server_error() || persisted.status == StatusCode::CONFLICT {
        JournalOutcome::FaultStatus
    } else {
        JournalOutcome::Delivered
    };
    let journal = journal_request(&parts, &body, Route::Run, persisted.sequence, checkpoints);
    record(&state, journal, persisted.status, outcome).await;
    response(persisted)
}

fn response(persisted: PersistedRun) -> Response {
    let mut response = match persisted.creation {
        Some(creation) => json_response(persisted.status, &creation),
        None if persisted.status == StatusCode::CONFLICT => {
            error_response(persisted.status, "idempotency_conflict")
        }
        None => error_response(persisted.status, "internal_error"),
    };
    if persisted.drop_response {
        response.headers_mut().insert(
            HeaderName::from_static(DROP_RESPONSE_HEADER),
            HeaderValue::from_static("1"),
        );
    }
    response
}

fn prepare_run(headers: &HeaderMap, body: &[u8]) -> Result<PreparedRun, RunParseError> {
    let key = idempotency_key(headers).map_err(|()| RunParseError {
        status: StatusCode::BAD_REQUEST,
        code: "invalid_idempotency_key",
    })?;
    let payload: RunRequest = serde_json::from_slice(body).map_err(|_| RunParseError {
        status: StatusCode::BAD_REQUEST,
        code: "invalid_request",
    })?;
    let workflow_name = payload.workflow_name.ok_or(RunParseError {
        status: StatusCode::BAD_REQUEST,
        code: "workflow_name_required",
    })?;
    let workflow_source = workflow_source(&workflow_name).ok_or(RunParseError {
        status: StatusCode::NOT_FOUND,
        code: "workflow_not_found",
    })?;
    let fingerprint =
        run_fingerprint(&workflow_name, payload.params.as_ref()).map_err(|_| RunParseError {
            status: StatusCode::BAD_REQUEST,
            code: "invalid_request",
        })?;
    Ok(PreparedRun {
        key,
        workflow_name,
        workflow_source,
        params: payload.params,
        fingerprint,
    })
}

async fn persist_run(
    state: &SharedState,
    prepared: &PreparedRun,
) -> Result<PersistedRun, HarnessError> {
    let mut inner = state.inner.lock().await;
    let sequence = inner.begin(Route::Run);
    let classified = prepared.key.as_ref().and_then(|key| {
        inner.persistent.idempotency.get(key).map(|mapping| {
            (mapping.fingerprint == prepared.fingerprint).then(|| mapping.job_id.clone())
        })
    });
    let conflict = matches!(classified, Some(None));
    let result = match &classified {
        Some(Some(job_id)) => inner
            .persistent
            .jobs
            .get(job_id)
            .map(|job| (StatusCode::OK, job.creation_response())),
        Some(None) => None,
        None => Some(create_job(&mut inner, prepared)),
    };
    let (status, creation) = match (conflict, result) {
        (true, _) => (StatusCode::CONFLICT, None),
        (false, Some((status, creation))) => (status, Some(creation)),
        (false, None) => (StatusCode::INTERNAL_SERVER_ERROR, None),
    };
    if status == StatusCode::CREATED {
        state.persist_locked(&inner).await?;
    }
    let drop_response = status == StatusCode::CREATED
        && std::mem::take(&mut inner.faults.accept_then_drop_run_response);
    Ok(PersistedRun {
        sequence,
        status,
        creation,
        drop_response,
    })
}

fn create_job(inner: &mut RuntimeState, prepared: &PreparedRun) -> (StatusCode, CreateJobResponse) {
    inner.persistent.next_job += 1;
    let id = format!("00000000-0000-4000-8000-{:012}", inner.persistent.next_job);
    let record = JobRecord {
        response: JobResponse {
            id: id.clone(),
            status: JobStatus::Queued,
            created_at: "2026-09-02T00:00:00Z".to_owned(),
            started_at: None,
            completed_at: None,
            progress: None,
            error: None,
            workflow_name: prepared.workflow_name.clone(),
            workflow_source: prepared.workflow_source.to_owned(),
            params: prepared.params.clone(),
            rerun_of_job_id: None,
            duration_ms: None,
        },
    };
    let creation = record.creation_response();
    inner.persistent.jobs.insert(id.clone(), record);
    if let Some(key) = &prepared.key {
        inner.persistent.idempotency.insert(
            key.clone(),
            IdempotencyRecord {
                fingerprint: prepared.fingerprint.clone(),
                job_id: id,
            },
        );
    }
    (StatusCode::CREATED, creation)
}

fn idempotency_key(headers: &HeaderMap) -> Result<Option<String>, ()> {
    let mut values = headers.get_all("idempotency-key").iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(());
    }
    let raw = value.to_str().map_err(|_| ())?;
    if raw.is_empty() || raw.len() > 255 || !raw.as_bytes().iter().all(u8::is_ascii_graphic) {
        return Err(());
    }
    Ok(Some(raw.to_owned()))
}

fn workflow_source(name: &str) -> Option<&'static str> {
    if saved_workflows()
        .iter()
        .any(|workflow| workflow.filename == name)
    {
        Some("workflow")
    } else if preset_entries().iter().any(|preset| preset.id == name) {
        Some("preset")
    } else {
        None
    }
}
