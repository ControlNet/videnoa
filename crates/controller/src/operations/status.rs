use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;

use crate::domain::{
    ReadinessCheck, ReadinessResponse, ReadinessStatus, TaskStatusCount, TaskStatusCountsResponse,
};

use super::error::OperationsError;
use super::OperationsState;

pub(super) async fn counts(
    State(state): State<OperationsState>,
) -> Result<Json<TaskStatusCountsResponse>, OperationsError> {
    let counts = state
        .store
        .task_status_counts()
        .await
        .map_err(|_| OperationsError::Internal)?;
    let total = counts.iter().try_fold(0_u64, |total, (_, count)| {
        total.checked_add(*count).ok_or(OperationsError::Internal)
    })?;
    let items = counts
        .into_iter()
        .map(|(status, count)| TaskStatusCount { status, count })
        .collect();
    Ok(Json(TaskStatusCountsResponse { items, total }))
}

pub(super) async fn readiness(
    State(state): State<OperationsState>,
    headers: HeaderMap,
) -> Result<Response, OperationsError> {
    let authentication = match crate::auth::authenticate(&state.auth, &headers, Utc::now()).await {
        Ok(_) => state.auth.check_ready().is_ok(),
        Err(crate::auth::AuthError::Unauthorized | crate::auth::AuthError::Forbidden) => {
            return Err(OperationsError::Unauthorized);
        }
        Err(crate::auth::AuthError::RateLimited) => return Err(OperationsError::Unauthorized),
        Err(
            crate::auth::AuthError::InvalidPasswordHash
            | crate::auth::AuthError::PasswordHashing
            | crate::auth::AuthError::PasswordFile { .. }
            | crate::auth::AuthError::InvalidLifetime
            | crate::auth::AuthError::Persistence(_),
        ) => false,
    };
    let database = state.store.check_ready().await;
    let roots = state.paths.check_ready();
    let checks = vec![
        check("migrations", database.is_ok()),
        check("authentication", authentication),
        check("root_handles", roots.is_ok()),
    ];
    let ready = checks.iter().all(|check| check.ready);
    let status = if ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    let readiness = if ready {
        ReadinessStatus::Ready
    } else {
        ReadinessStatus::NotReady
    };
    Ok((
        status,
        Json(ReadinessResponse {
            status: readiness,
            checks,
        }),
    )
        .into_response())
}

fn check(name: &str, ready: bool) -> ReadinessCheck {
    ReadinessCheck {
        name: name.to_owned(),
        ready,
        message: (!ready).then(|| "unavailable".to_owned()),
    }
}
