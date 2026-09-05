use axum::extract::rejection::JsonRejection;
use axum::extract::State;
use axum::Json;
use chrono::Utc;
use std::net::SocketAddr;
use std::time::Duration;

use crate::config::{ConfigBootstrap, PreparedListener};
use crate::domain::{SettingsPaths, SettingsResponse, SettingsUpdateRequest, TaskActionRequest};
use crate::persistence::{CasOutcome, SettingsRecord};

use super::request_failure::OperationsError;
use super::OperationsState;

const PROJECTION_RETRY_DELAY: Duration = Duration::from_millis(250);

#[path = "settings/validate.rs"]
mod validate_request;

#[cfg(test)]
#[path = "settings/tests.rs"]
mod tests;

pub(super) async fn get(
    State(state): State<OperationsState>,
) -> Result<Json<SettingsResponse>, OperationsError> {
    current(&state).await.map(Json)
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
        .settings()
        .await
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
        .settings()
        .await
        .map_err(|_| OperationsError::Internal)?;
    if record.version != request.version {
        return Err(OperationsError::Conflict(
            "settings changed since they were read",
        ));
    }
    let config = validate_request::build_config(&state.config.paths, &request)?;
    let document = config.to_toml().map_err(|_| OperationsError::Internal)?;
    let prepared = prepare_listener(&record, &request, state.listener.is_some()).await?;
    let update = config
        .settings_update(request.version, &document, Utc::now())
        .map_err(|_| OperationsError::InvalidRequest)?;
    let _admission = state.scheduler.lock_settings().await;
    match state
        .store
        .update_configuration(&update)
        .await
        .map_err(|_| OperationsError::Internal)?
    {
        CasOutcome::Applied { new_version } => {
            let projected = projection_complete(state, &document, new_version).await;
            state
                .scheduler
                .apply_runtime(&update.scheduler, &update.timeouts, &update.retry)
                .map_err(|error| OperationsError::from_scheduler(&error))?;
            state
                .auth
                .reconfigure(config.auth.clone())
                .map_err(|error| OperationsError::from_auth(&error))?;
            if let (Some(prepared), Some(listener)) = (prepared, &state.listener) {
                listener
                    .handoff(prepared)
                    .await
                    .map_err(|_| OperationsError::Internal)?;
            }
            if !projected {
                schedule_projection_repair(state, document, new_version);
                return Err(OperationsError::CommittedDegraded);
            }
        }
        CasOutcome::Conflict => {
            return Err(OperationsError::Conflict(
                "settings changed since they were read",
            ));
        }
    }
    current(state).await.map(Json)
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

async fn projection_complete(state: &OperationsState, document: &str, version: u64) -> bool {
    if ConfigBootstrap::repair_projection(&state.workspace, document).is_err() {
        return false;
    }
    state
        .store
        .complete_config_projection(version)
        .await
        .is_ok_and(|completed| completed)
}

fn schedule_projection_repair(state: &OperationsState, document: String, version: u64) {
    let store = state.store.clone();
    let workspace = state.workspace.clone();
    let settings_lock = state.settings_lock.clone();
    let shutdown = state.shutdown.clone();
    tokio::spawn(async move {
        loop {
            let repaired = {
                let serialized = settings_lock.lock().await;
                let Ok(record) = store.settings().await else {
                    drop(serialized);
                    if !wait_for_projection_retry(shutdown.as_ref()).await {
                        return;
                    }
                    continue;
                };
                if record.version != version
                    || record.pending_config_document.as_deref() != Some(document.as_str())
                {
                    return;
                }
                if ConfigBootstrap::repair_projection(&workspace, &document).is_err() {
                    false
                } else {
                    store
                        .complete_config_projection(version)
                        .await
                        .is_ok_and(|completed| completed)
                }
            };
            if repaired {
                return;
            }
            if !wait_for_projection_retry(shutdown.as_ref()).await {
                return;
            }
        }
    });
}

async fn wait_for_projection_retry(shutdown: Option<&tokio_util::sync::CancellationToken>) -> bool {
    if let Some(token) = shutdown {
        tokio::select! {
            () = token.cancelled() => false,
            () = tokio::time::sleep(PROJECTION_RETRY_DELAY) => true,
        }
    } else {
        tokio::time::sleep(PROJECTION_RETRY_DELAY).await;
        true
    }
}

async fn current(state: &OperationsState) -> Result<SettingsResponse, OperationsError> {
    let record = state
        .store
        .settings()
        .await
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
