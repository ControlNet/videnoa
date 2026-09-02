use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::Utc;
use serde::Deserialize;

use crate::domain::{
    TaskActionRequest, WorkerCreateRequest, WorkerDeleteResponse, WorkerId, WorkerListResponse,
    WorkerSummary, WorkerUpdateRequest,
};
use crate::persistence::WorkerRecord;

use super::error::OperationsError;
use super::OperationsState;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct VersionQuery {
    version: u64,
}

pub(super) async fn list(
    State(state): State<OperationsState>,
) -> Result<Json<WorkerListResponse>, OperationsError> {
    let records = state
        .store
        .workers()
        .await
        .map_err(|_| OperationsError::Internal)?;
    let total = u64::try_from(records.len()).map_err(|_| OperationsError::Internal)?;
    let mut items = Vec::with_capacity(records.len());
    for record in records {
        items.push(summary(&state, record).await?);
    }
    Ok(Json(WorkerListResponse { items, total }))
}

pub(super) async fn create(
    State(state): State<OperationsState>,
    payload: Result<Json<WorkerCreateRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<WorkerSummary>), OperationsError> {
    let Json(request) = payload.map_err(|_| OperationsError::InvalidRequest)?;
    let record = state
        .workers
        .create(request, Utc::now())
        .await
        .map_err(|error| OperationsError::from_worker(&error))?;
    let worker = summary(&state, record).await?;
    Ok((StatusCode::CREATED, Json(worker)))
}

pub(super) async fn update(
    State(state): State<OperationsState>,
    id: Result<Path<WorkerId>, PathRejection>,
    payload: Result<Json<WorkerUpdateRequest>, JsonRejection>,
) -> Result<Json<WorkerSummary>, OperationsError> {
    let Path(id) = id.map_err(|_| OperationsError::InvalidRequest)?;
    let Json(request) = payload.map_err(|_| OperationsError::InvalidRequest)?;
    let record = state
        .workers
        .update(id, request, Utc::now())
        .await
        .map_err(|error| OperationsError::from_worker(&error))?;
    changed(&state, record).await
}

pub(super) async fn enable(
    state: State<OperationsState>,
    id: Result<Path<WorkerId>, PathRejection>,
    payload: Result<Json<TaskActionRequest>, JsonRejection>,
) -> Result<Json<WorkerSummary>, OperationsError> {
    set_enabled(state, id, payload, true).await
}

pub(super) async fn disable(
    state: State<OperationsState>,
    id: Result<Path<WorkerId>, PathRejection>,
    payload: Result<Json<TaskActionRequest>, JsonRejection>,
) -> Result<Json<WorkerSummary>, OperationsError> {
    set_enabled(state, id, payload, false).await
}

async fn set_enabled(
    State(state): State<OperationsState>,
    id: Result<Path<WorkerId>, PathRejection>,
    payload: Result<Json<TaskActionRequest>, JsonRejection>,
    enabled: bool,
) -> Result<Json<WorkerSummary>, OperationsError> {
    let Path(id) = id.map_err(|_| OperationsError::InvalidRequest)?;
    let Json(request) = payload.map_err(|_| OperationsError::InvalidRequest)?;
    let record = state
        .workers
        .set_enabled(id, request.version, enabled, Utc::now())
        .await
        .map_err(|error| OperationsError::from_worker(&error))?;
    changed(&state, record).await
}

pub(super) async fn delete(
    State(state): State<OperationsState>,
    id: Result<Path<WorkerId>, PathRejection>,
    query: Result<Query<VersionQuery>, QueryRejection>,
) -> Result<Json<WorkerDeleteResponse>, OperationsError> {
    let Path(id) = id.map_err(|_| OperationsError::InvalidRequest)?;
    let Query(query) = query.map_err(|_| OperationsError::InvalidRequest)?;
    state
        .workers
        .delete(id, query.version)
        .await
        .map_err(|error| OperationsError::from_worker(&error))?;
    Ok(Json(WorkerDeleteResponse {
        worker_id: id,
        deleted: true,
    }))
}

async fn changed(
    state: &OperationsState,
    record: WorkerRecord,
) -> Result<Json<WorkerSummary>, OperationsError> {
    summary(state, record).await.map(Json)
}

pub(super) async fn summary(
    state: &OperationsState,
    record: WorkerRecord,
) -> Result<WorkerSummary, OperationsError> {
    let capacity = state
        .workers
        .capacity(record.id)
        .await
        .map_err(|error| OperationsError::from_worker(&error))?;
    Ok(WorkerSummary {
        id: record.id,
        version: record.version,
        name: record.name,
        api_url: record.api_url,
        enabled: record.enabled,
        online: record.online,
        compute_slots: record.compute_slots,
        capabilities: record.capabilities,
        capacity,
        last_seen_at: record.last_seen_at,
        last_assigned_at: record.last_assigned_at,
        created_at: record.created_at,
        updated_at: record.updated_at,
        last_error: record.last_error,
    })
}
