use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;

use crate::auth::{authenticate, authorize_mutation, AuthService};
use crate::domain::{
    FieldErrorCode, IdempotencyKey, PageRequest, Task, TaskCreateRequest, TaskDetailResponse,
    TaskId, TaskListQuery, TaskListResponse,
};

use super::error::TaskApiError;
use super::intake::{IntakeOutcome, TaskService};
use super::mapping;

const IDEMPOTENCY_HEADER: &str = "idempotency-key";

#[derive(Clone)]
struct TaskRouteState {
    auth: AuthService,
    tasks: TaskService,
}

pub(crate) fn router(auth: AuthService, tasks: TaskService) -> Router {
    let state = TaskRouteState { auth, tasks };
    let reads = Router::new()
        .route("/api/tasks", get(list_tasks))
        .route("/api/tasks/{id}", get(task_detail))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_auth));
    let writes = Router::new()
        .route("/api/tasks", post(create_task))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_mutation,
        ));
    reads.merge(writes).with_state(state)
}

async fn require_auth(
    State(state): State<TaskRouteState>,
    request: Request,
    next: Next,
) -> Result<Response, TaskApiError> {
    authenticate(&state.auth, request.headers(), Utc::now())
        .await
        .map_err(|error| TaskApiError::from_auth(&error))?;
    Ok(next.run(request).await)
}

async fn require_mutation(
    State(state): State<TaskRouteState>,
    request: Request,
    next: Next,
) -> Result<Response, TaskApiError> {
    authorize_mutation(&state.auth, request.headers(), Utc::now())
        .await
        .map_err(|error| TaskApiError::from_auth(&error))?;
    Ok(next.run(request).await)
}

async fn create_task(
    State(state): State<TaskRouteState>,
    headers: HeaderMap,
    payload: Result<Json<TaskCreateRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<Task>), TaskApiError> {
    let key = idempotency_key(&headers)?;
    let Json(request) = payload.map_err(|_| TaskApiError::InvalidRequest)?;
    match state.tasks.create(key, request).await? {
        IntakeOutcome::Created(task) => Ok((StatusCode::CREATED, Json(task))),
        IntakeOutcome::Replayed(task) => Ok((StatusCode::OK, Json(task))),
    }
}

async fn list_tasks(
    State(state): State<TaskRouteState>,
    query: Result<Query<TaskListQuery>, QueryRejection>,
) -> Result<Json<TaskListResponse>, TaskApiError> {
    let Query(query) = query.map_err(|_| TaskApiError::InvalidRequest)?;
    let page = state
        .tasks
        .store()
        .task_page(&query)
        .await
        .map_err(|_| TaskApiError::Internal)?;
    Ok(Json(mapping::list(
        page,
        query.page.limit().get(),
        query.page.offset().get(),
    )))
}

async fn task_detail(
    State(state): State<TaskRouteState>,
    id: Result<Path<TaskId>, PathRejection>,
    query: Result<Query<PageRequest>, QueryRejection>,
) -> Result<Json<TaskDetailResponse>, TaskApiError> {
    let Path(id) = id.map_err(|_| TaskApiError::InvalidRequest)?;
    let Query(page_request) = query.map_err(|_| TaskApiError::InvalidRequest)?;
    let task = state
        .tasks
        .store()
        .task(id)
        .await
        .map_err(|_| TaskApiError::Internal)?
        .ok_or(TaskApiError::NotFound)?;
    let attempts = state
        .tasks
        .store()
        .task_attempt_page(id, page_request)
        .await
        .map_err(|_| TaskApiError::Internal)?;
    Ok(Json(mapping::detail(
        task,
        attempts,
        page_request.limit().get(),
        page_request.offset().get(),
    )))
}

fn idempotency_key(headers: &HeaderMap) -> Result<IdempotencyKey, TaskApiError> {
    let mut values = headers.get_all(IDEMPOTENCY_HEADER).iter();
    let value = values.next().ok_or_else(|| {
        TaskApiError::invalid(
            "idempotency_key",
            FieldErrorCode::Required,
            "exactly one Idempotency-Key header is required",
        )
    })?;
    if values.next().is_some() {
        return Err(TaskApiError::invalid(
            "idempotency_key",
            FieldErrorCode::InvalidValue,
            "exactly one Idempotency-Key header is required",
        ));
    }
    let value = value.to_str().map_err(|_| {
        TaskApiError::invalid(
            "idempotency_key",
            FieldErrorCode::InvalidValue,
            "Idempotency-Key must contain visible ASCII bytes",
        )
    })?;
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 255 || !bytes.iter().all(u8::is_ascii_graphic) {
        return Err(TaskApiError::invalid(
            "idempotency_key",
            FieldErrorCode::InvalidValue,
            "Idempotency-Key must contain 1 to 255 visible ASCII bytes",
        ));
    }
    Ok(IdempotencyKey::new(value))
}
