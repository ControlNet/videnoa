use axum::extract::rejection::JsonRejection;
use axum::extract::State;
use axum::Json;
use chrono::Utc;
use std::net::SocketAddr;

use crate::config::PreparedListener;
use crate::domain::{SettingsPaths, SettingsResponse, SettingsUpdateRequest, TaskActionRequest};
use crate::persistence::{CasOutcome, SettingsRecord};

use super::request_failure::OperationsError;
use super::OperationsState;

#[path = "settings/validate.rs"]
mod validate_request;

#[cfg(test)]
#[path = "settings/tests.rs"]
mod tests;

pub(super) async fn get(
    State(state): State<OperationsState>,
) -> Result<Json<SettingsResponse>, OperationsError> {
    current(&state).map(Json)
}

pub(super) async fn update(
    State(state): State<OperationsState>,
    payload: Result<Json<SettingsUpdateRequest>, JsonRejection>,
) -> Result<Json<SettingsResponse>, OperationsError> {
    let Json(request) = payload.map_err(|_| OperationsError::InvalidRequest)?;
    validate_request::validate(&request)?;
    apply(&state, request).await
}

pub(super) async fn pause(
    State(state): State<OperationsState>,
    payload: Result<Json<TaskActionRequest>, JsonRejection>,
) -> Result<Json<SettingsResponse>, OperationsError> {
    set_paused(&state, payload, true).await
}

pub(super) async fn resume(
    State(state): State<OperationsState>,
    payload: Result<Json<TaskActionRequest>, JsonRejection>,
) -> Result<Json<SettingsResponse>, OperationsError> {
    set_paused(&state, payload, false).await
}

async fn set_paused(
    state: &OperationsState,
    payload: Result<Json<TaskActionRequest>, JsonRejection>,
    paused: bool,
) -> Result<Json<SettingsResponse>, OperationsError> {
    let Json(action) = payload.map_err(|_| OperationsError::InvalidRequest)?;
    let record = state
        .store
        .config_manager()
        .settings()
        .map_err(|_| OperationsError::Internal)?;
    let mut scheduler = record.scheduler;
    scheduler.paused = paused;
    apply(
        state,
        SettingsUpdateRequest {
            version: action.version,
            server: record.server,
            auth: record.auth,
            scheduler,
            timeouts: record.timeouts,
            retry: record.retry,
        },
    )
    .await
}

async fn apply(
    state: &OperationsState,
    request: SettingsUpdateRequest,
) -> Result<Json<SettingsResponse>, OperationsError> {
    let _serialized = state.settings_lock.lock().await;
    let record = state
        .store
        .config_manager()
        .settings()
        .map_err(|_| OperationsError::Internal)?;
    if record.version != request.version {
        return Err(OperationsError::Conflict(
            "settings changed since they were read",
        ));
    }
    let config = validate_request::build_config(&state.config.paths, &request)?;
    let prepared = prepare_listener(&record, &request, state.listener.is_some()).await?;
    let handoff = match (prepared, &state.listener) {
        (Some(prepared), Some(listener)) => Some(
            listener
                .prepare_handoff(prepared)
                .await
                .map_err(|_| OperationsError::InvalidField("server", "HTTP listener stopped"))?,
        ),
        _ => None,
    };
    let update = config
        .settings_update(request.version, Utc::now())
        .map_err(|_| OperationsError::InvalidRequest)?;
    let _admission = state.scheduler.lock_settings().await;
    // All policy validation and listener binding precede the single durable write.
    match state
        .store
        .config_manager()
        .commit(config.clone(), request.version)
        .map_err(|_| OperationsError::Internal)?
    {
        CasOutcome::Conflict => {
            return Err(OperationsError::Conflict(
                "settings changed since they were read",
            ))
        }
        CasOutcome::Applied { .. } => {}
    }
    state
        .scheduler
        .apply_runtime(&update.scheduler, &update.timeouts, &update.retry)
        .map_err(|error| OperationsError::from_scheduler(&error))?;
    state
        .auth
        .reconfigure(config.auth)
        .map_err(|error| OperationsError::from_auth(&error))?;
    if let Some(handoff) = handoff {
        handoff
            .apply()
            .await
            .map_err(|_| OperationsError::CommittedDegraded)?;
    }
    current(state).map(Json)
}

async fn prepare_listener(
    current: &SettingsRecord,
    request: &SettingsUpdateRequest,
    listener_available: bool,
) -> Result<Option<PreparedListener>, OperationsError> {
    if current.server == request.server {
        return Ok(None);
    }
    if !listener_available {
        return Err(OperationsError::InvalidField(
            "server",
            "live listener reconfiguration is unavailable",
        ));
    }
    PreparedListener::bind(SocketAddr::new(request.server.host, request.server.port))
        .await
        .map(Some)
        .map_err(|_| OperationsError::InvalidField("server", "address could not be bound"))
}

fn current(state: &OperationsState) -> Result<SettingsResponse, OperationsError> {
    let record = state
        .store
        .config_manager()
        .settings()
        .map_err(|_| OperationsError::Internal)?;
    Ok(response(state, record))
}

fn response(state: &OperationsState, record: SettingsRecord) -> SettingsResponse {
    SettingsResponse {
        version: record.version,
        paths: SettingsPaths {
            workspace: state.workspace.clone(),
            data_root: state.config.paths.data_root.clone(),
            config_file: state.workspace.join("data/controller.toml"),
        },
        server: record.server,
        secure_cookie: record.auth.secure_cookie,
        session_absolute_seconds: record.auth.session_absolute_seconds,
        session_idle_seconds: record.auth.session_idle_seconds,
        scheduler: record.scheduler,
        timeouts: record.timeouts,
        retry: record.retry,
    }
}
