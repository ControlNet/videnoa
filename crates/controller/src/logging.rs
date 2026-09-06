//! Console diagnostics for the standalone Controller process.

use tracing_subscriber::EnvFilter;

const DEFAULT_FILTER: &str = "warn,videnoa_controller=info";

/// Installs timestamped console logging, captured by `docker logs`.
/// Invalid `RUST_LOG` values fall back to the operational default.
pub fn init() {
    let configured = std::env::var("RUST_LOG").ok();
    let (filter, invalid) = filter(configured.as_deref());
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .init();
    if invalid {
        tracing::warn!("Invalid RUST_LOG; using default Controller log levels");
    }
}

fn filter(value: Option<&str>) -> (EnvFilter, bool) {
    match value.filter(|value| !value.trim().is_empty()) {
        Some(value) => match EnvFilter::try_new(value) {
            Ok(filter) => (filter, false),
            Err(_) => (EnvFilter::new(DEFAULT_FILTER), true),
        },
        None => (EnvFilter::new(DEFAULT_FILTER), false),
    }
}

pub(crate) fn task_created(task: &crate::persistence::NewTask) {
    // Request paths, source references and keys can contain private data.
    tracing::info!(task_id = %task.id, source = ?task.request.source,
        priority = task.request.priority, input_bytes = task.input_size, "Task created");
}

/// Diagnostics explicitly supplied by task handlers, never extracted from response bodies.
#[derive(Clone)]
pub(crate) struct TaskRequestDiagnostics(pub(crate) String);

/// Logs response status and latency without bodies, headers, or raw URLs.
pub async fn request(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let method = request.method().clone();
    let route = request
        .extensions()
        .get::<axum::extract::MatchedPath>()
        .map_or("unmatched", axum::extract::MatchedPath::as_str)
        .to_owned();
    let started = std::time::Instant::now();
    let response = next.run(request).await;
    let status = response.status().as_u16();
    let elapsed_ms = started.elapsed().as_millis();
    let diagnostics = response
        .extensions()
        .get::<TaskRequestDiagnostics>()
        .map_or("", |diagnostics| diagnostics.0.as_str());
    if status >= 500 {
        tracing::error!(%method, %route, status, elapsed_ms, diagnostics, "HTTP request failed");
    } else if status >= 400 || status == 207 {
        tracing::warn!(%method, %route, status, elapsed_ms, diagnostics, "HTTP request rejected");
    } else if method == axum::http::Method::GET || method == axum::http::Method::HEAD {
        tracing::debug!(%method, %route, status, elapsed_ms, diagnostics, "HTTP request completed");
    } else {
        tracing::info!(%method, %route, status, elapsed_ms, diagnostics, "HTTP request completed");
    }
    response
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<u8>>>);

    impl Write for Capture {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn console_defaults_show_operations_without_polling_or_dependency_noise() {
        let capture = Capture::default();
        let writer = capture.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_env_filter(super::filter(None).0)
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(task_id = "test-task", "Task stage changed");
            tracing::warn!("Transfer retry scheduled");
            tracing::debug!("poll tick");
            tracing::info!(target: "sqlx", "query details");
        });
        let output = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
        assert!(output.contains("INFO"));
        assert!(output.contains("task_id=\"test-task\""));
        assert!(output.contains("Transfer retry scheduled"));
        assert!(!output.contains("poll tick"));
        assert!(!output.contains("query details"));
        assert!(!output.contains('\u{1b}'));
    }

    #[test]
    fn invalid_filter_falls_back_and_valid_override_is_accepted() {
        assert!(super::filter(Some("[invalid")).1);
        assert!(!super::filter(Some("videnoa_controller=debug")).1);
        assert!(!super::filter(Some("")).1);
    }
    #[tokio::test]
    async fn committed_creation_is_logged_once_without_private_request_data() {
        use crate::domain::*;
        use crate::persistence::*;
        use tracing::instrument::WithSubscriber;

        let directory = tempfile::tempdir().unwrap();
        let database = Database::open(DatabaseOptions::new(directory.path().join("test.sqlite3")))
            .await
            .unwrap();
        let store = Store::new(database);
        let now = chrono::Utc::now();
        let mut task = NewTask {
            id: TaskId::random(),
            request: TaskCreateRequest {
                input_path: InputPath::new("/private-input/video.mkv"),
                output_path: OutputPath::new("/private-output/video.mp4"),
                workflow: WorkflowName::new("test-workflow"),
                priority: 1,
                source: TaskSource::Api,
                source_reference: Some(SourceReference::new("private-reference")),
            },
            input_extension: InputExtension::new("mkv"),
            output_extension: OutputExtension::new("mp4"),
            input_size: 4096,
            input_mtime: now,
            input_identity: InputIdentity::new([1; 16]),
            input_content_identity: InputContentIdentity::new([2; 16]),
            created_at: now,
        };
        let record = IdempotencyRecord {
            key: IdempotencyKey::new("private-idempotency-value"),
            request_fingerprint: [3; 32],
            task_id: task.id,
            created_at: now,
        };
        let capture = Capture::default();
        let writer = capture.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_env_filter(super::filter(None).0)
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        let original_id = task.id;
        async {
            assert_eq!(
                store
                    .insert_task_with_idempotency(&task, &record)
                    .await
                    .unwrap(),
                TaskIngressOutcome::Inserted
            );
            task.id = TaskId::random();
            assert_eq!(
                store
                    .insert_task_with_idempotency(&task, &record)
                    .await
                    .unwrap(),
                TaskIngressOutcome::Replay(original_id)
            );
        }
        .with_subscriber(subscriber)
        .await;
        let output = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
        assert_eq!(output.matches("Task created").count(), 1);
        assert!(output.contains(&original_id.to_string()));
        assert!(!output.contains(&task.id.to_string()));
        assert!(!output.contains("private-"));
        assert!(store.task(task.id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn http_diagnostics_use_route_templates_and_hide_query_and_headers() {
        use axum::{body::Body, http::Request, middleware, routing::post, Router};
        use tower::ServiceExt;
        use tracing::instrument::WithSubscriber;

        let capture = Capture::default();
        let writer = capture.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        let app = Router::new()
            .route(
                "/tasks/{id}",
                post(|| async { axum::http::StatusCode::CONFLICT }),
            )
            .layer(middleware::from_fn(super::request));
        let response = app
            .oneshot(
                Request::post("/tasks/private-id?token=private-query")
                    .header("authorization", "Bearer private-auth")
                    .body(Body::from("private-body"))
                    .unwrap(),
            )
            .with_subscriber(subscriber)
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::CONFLICT);
        let output = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
        assert!(output.contains("/tasks/{id}"));
        assert!(output.contains("status=409"));
        assert!(output.contains("elapsed_ms="));
        assert!(!output.contains("private-"));
    }
    #[tokio::test]
    async fn task_failure_details_and_partial_batch_failures_are_visible_at_default_level() {
        use axum::{
            body::Body, http::Request, middleware, response::IntoResponse, routing::post, Router,
        };
        use tower::ServiceExt;
        use tracing::instrument::WithSubscriber;

        for status in [400, 401, 409, 500, 207] {
            let capture = Capture::default();
            let writer = capture.clone();
            let subscriber = tracing_subscriber::fmt()
                .with_env_filter(super::filter(None).0)
                .with_ansi(false)
                .with_writer(move || writer.clone())
                .finish();
            let app = Router::new().route("/api/tasks", post(move || async move {
                let mut response = axum::http::StatusCode::from_u16(status).unwrap().into_response();
                response.extensions_mut().insert(super::TaskRequestDiagnostics(
                    serde_json::json!({"error": {"code": "invalid_request", "message": "request validation failed",
                        "field_errors": [{"field": "input_path", "message": "input is unavailable"}]}}).to_string(),
                ));
                response
            })).layer(middleware::from_fn(super::request));
            app.oneshot(Request::post("/api/tasks").body(Body::empty()).unwrap())
                .with_subscriber(subscriber)
                .await
                .unwrap();
            let output = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
            assert!(output.contains(if status == 500 { "ERROR" } else { "WARN" }));
            for expected in [
                "invalid_request",
                "request validation failed",
                "input_path",
                "input is unavailable",
            ] {
                assert!(output.contains(expected), "missing {expected}: {output}");
            }
        }
    }
}
