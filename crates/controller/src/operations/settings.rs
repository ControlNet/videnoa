use axum::extract::rejection::JsonRejection;
use axum::extract::State;
use axum::Json;
use chrono::Utc;

use crate::domain::{SettingsPaths, SettingsResponse, SettingsUpdateRequest, TaskActionRequest};
use crate::persistence::{SettingsRecord, SettingsUpdate};

use super::error::OperationsError;
use super::OperationsState;

const MAX_DURATION_SECONDS: u64 = 7 * 24 * 60 * 60;
const MAX_RETRY_ATTEMPTS: u32 = 100;

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
    validate(&request)?;
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
    let current = state
        .store
        .settings()
        .await
        .map_err(|_| OperationsError::Internal)?;
    let mut scheduler = current.scheduler;
    scheduler.paused = paused;
    apply(
        state,
        SettingsUpdateRequest {
            version: action.version,
            scheduler,
            timeouts: current.timeouts,
            retry: current.retry,
        },
    )
    .await
}

async fn apply(
    state: &OperationsState,
    request: SettingsUpdateRequest,
) -> Result<Json<SettingsResponse>, OperationsError> {
    state
        .scheduler
        .update_settings(SettingsUpdate {
            expected_version: request.version,
            scheduler: request.scheduler,
            timeouts: request.timeouts,
            retry: request.retry,
            updated_at: Utc::now(),
        })
        .await
        .map_err(|error| OperationsError::from_scheduler(&error))?;
    current(state).await.map(Json)
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
            input_roots: state.config.paths.input_roots.clone(),
            output_roots: state.config.paths.output_roots.clone(),
            data_root: state.config.paths.data_root.clone(),
            temp_root: state.config.paths.temp_root.clone(),
            password_hash_file: state.config.auth.password_hash_file.clone(),
        },
        secure_cookie: state.auth.secure_cookie(),
        session_absolute_seconds: state.auth.session_absolute_seconds(),
        session_idle_seconds: state.auth.session_idle_seconds(),
        scheduler: record.scheduler,
        timeouts: record.timeouts,
        retry: record.retry,
    }
}

fn validate(request: &SettingsUpdateRequest) -> Result<(), OperationsError> {
    if request.timeouts.health_seconds == 0 {
        return Err(OperationsError::InvalidField(
            "health_seconds",
            "value must be greater than zero",
        ));
    }
    if request.timeouts.health_seconds > MAX_DURATION_SECONDS {
        return Err(OperationsError::InvalidField(
            "health_seconds",
            "value must not exceed seven days",
        ));
    }
    if request.timeouts.poll_seconds == 0 {
        return Err(OperationsError::InvalidField(
            "poll_seconds",
            "value must be greater than zero",
        ));
    }
    if request.timeouts.poll_seconds > MAX_DURATION_SECONDS {
        return Err(OperationsError::InvalidField(
            "poll_seconds",
            "value must not exceed seven days",
        ));
    }
    if request.timeouts.transfer_seconds == 0 {
        return Err(OperationsError::InvalidField(
            "transfer_seconds",
            "value must be greater than zero",
        ));
    }
    if request.timeouts.transfer_seconds > MAX_DURATION_SECONDS {
        return Err(OperationsError::InvalidField(
            "transfer_seconds",
            "value must not exceed seven days",
        ));
    }
    if request.retry.initial_seconds == 0 || request.retry.maximum_seconds == 0 {
        return Err(OperationsError::InvalidField(
            "retry",
            "retry delays must be greater than zero",
        ));
    }
    if request.retry.initial_seconds > request.retry.maximum_seconds {
        return Err(OperationsError::InvalidField(
            "retry",
            "initial delay must not exceed maximum delay",
        ));
    }
    if request.retry.maximum_seconds > MAX_DURATION_SECONDS {
        return Err(OperationsError::InvalidField(
            "retry",
            "retry delays must not exceed seven days",
        ));
    }
    if request.retry.max_attempts == 0 {
        return Err(OperationsError::InvalidField(
            "max_attempts",
            "value must be greater than zero",
        ));
    }
    if request.retry.max_attempts > MAX_RETRY_ATTEMPTS {
        return Err(OperationsError::InvalidField(
            "max_attempts",
            "value must not exceed 100",
        ));
    }
    Ok(())
}
