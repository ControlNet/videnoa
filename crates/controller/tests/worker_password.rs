use chrono::Utc;
use serde_json::json;
use std::error::Error;
use tempfile::TempDir;
use videnoa_controller::domain::{WorkerCreateRequest, WorkerUpdateRequest};
use videnoa_controller::persistence::{Database, DatabaseOptions, Store};
use videnoa_controller::workers::WorkerRegistry;

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

#[tokio::test]
async fn worker_password_create_keep_replace_clear_and_restart() -> TestResult {
    let dir = TempDir::new()?;
    let path = dir.path().join("controller.sqlite3");
    let database = Database::open(DatabaseOptions::new(&path)).await?;
    let store = Store::new(database.clone());
    let registry = WorkerRegistry::new(store.clone());
    let credential = uuid::Uuid::new_v4().to_string();
    let base = json!({"name":"protected", "api_url":"http://127.0.0.1:13000/", "enabled":true, "compute_slots":1});
    let mut create = base.clone();
    create["password"] = json!(credential);
    let created = registry
        .create(
            serde_json::from_value::<WorkerCreateRequest>(create)?,
            Utc::now(),
        )
        .await?;
    assert_eq!(
        created
            .password
            .as_ref()
            .map(videnoa_controller::domain::SecretString::expose),
        Some(credential.as_str())
    );
    assert!(!format!("{created:?}").contains(&credential));
    let mut update = base.clone();
    update["version"] = json!(created.version);
    let kept = registry
        .update(
            created.id,
            serde_json::from_value::<WorkerUpdateRequest>(update.clone())?,
            Utc::now(),
        )
        .await?;
    assert_eq!(kept.password, created.password);
    update["version"] = json!(kept.version);
    update["password"] = json!("短");
    let replaced = registry
        .update(
            created.id,
            serde_json::from_value(update.clone())?,
            Utc::now(),
        )
        .await?;
    assert_eq!(
        replaced
            .password
            .as_ref()
            .map(videnoa_controller::domain::SecretString::expose),
        Some("短")
    );
    assert!(registry
        .update(
            created.id,
            serde_json::from_value(update.clone())?,
            Utc::now()
        )
        .await
        .is_err());
    database.pool().close().await;
    let database = Database::open(DatabaseOptions::new(&path)).await?;
    let store = Store::new(database);
    let registry = WorkerRegistry::new(store.clone());
    let restored = store.worker(created.id).await?.ok_or("worker missing")?;
    assert_eq!(restored.password, replaced.password);
    update["version"] = json!(restored.version);
    update["password"] = serde_json::Value::Null;
    let cleared = registry
        .update(created.id, serde_json::from_value(update)?, Utc::now())
        .await?;
    assert!(cleared.password.is_none());
    Ok(())
}

#[tokio::test]
async fn only_protected_remote_requests_carry_the_worker_credential() -> TestResult {
    use axum::{
        extract::State,
        http::{HeaderMap, StatusCode},
        routing::any,
        Router,
    };
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::time::Duration;
    use videnoa_controller::{
        domain::{SecretString, WorkerApiUrl},
        remote::{FileApiPath, PayloadLimits, RemoteTimeouts, VidenoaClient},
    };
    let secret = SecretString::new(uuid::Uuid::new_v4().to_string());
    let count = Arc::new(AtomicUsize::new(0));
    let state = (secret.clone(), count.clone());
    let router = Router::new()
        .route(
            "/{*path}",
            any(
                |State((secret, count)): State<(SecretString, Arc<AtomicUsize>)>,
                 uri: axum::http::Uri,
                 headers: HeaderMap| async move {
                    if uri.path() == "/api/health" {
                        assert!(!headers.contains_key("authorization"));
                        return StatusCode::NOT_FOUND;
                    }
                    if headers
                        .get("authorization")
                        .map(axum::http::HeaderValue::as_bytes)
                        == Some(format!("Bearer {}", secret.expose()).as_bytes())
                    {
                        count.fetch_add(1, Ordering::SeqCst);
                        StatusCode::NOT_FOUND
                    } else {
                        StatusCode::UNAUTHORIZED
                    }
                },
            ),
        )
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = WorkerApiUrl::parse(&format!("http://{}", listener.local_addr()?))?;
    let server = tokio::spawn(async move { axum::serve(listener, router).await });
    let client = VidenoaClient::new_with_password(
        url,
        RemoteTimeouts::new(
            Duration::from_secs(3),
            Duration::from_secs(3),
            Duration::from_secs(3),
        )?,
        PayloadLimits::new(4096, 1024)?,
        Some(&secret),
    )?;
    let path = FileApiPath::parse("test/input.bin")?;
    let _ = client.health().await;
    let _ = client.workflows().await;
    let _ = client.presets().await;
    let _ = client
        .workflow_interface(&videnoa_controller::domain::WorkflowName::new("test"))
        .await;
    let _ = client.stat(&path).await;
    let _ = client.upload(&path, 0, tokio::io::empty()).await;
    let _ = client.delete_file(&path).await;
    let _ = client.download(&path, &mut tokio::io::sink()).await;
    let id = videnoa_controller::domain::RemoteJobId::random();
    let _ = client.job(id).await;
    let _ = client.cancel_job(id).await;
    let _ = client
        .run(
            &videnoa_controller::domain::WorkflowName::new("test"),
            videnoa_controller::domain::SubmissionKey::random(),
            &std::collections::BTreeMap::new(),
        )
        .await;
    server.abort();
    assert_eq!(count.load(Ordering::SeqCst), 10);
    Ok(())
}

#[test]
fn production_worker_clients_cannot_silently_become_anonymous() -> TestResult {
    fn audit(path: &std::path::Path) -> TestResult {
        for entry in std::fs::read_dir(path)? {
            let path = entry?.path();
            if path.is_dir() {
                audit(&path)?;
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let source = std::fs::read_to_string(&path)?;
                let compact: String = source.chars().filter(|c| !c.is_whitespace()).collect();
                assert!(
                    !compact.contains("VidenoaClient::new("),
                    "anonymous production client: {}",
                    path.display()
                );
                if !path.components().any(|part| part.as_os_str() == "remote") {
                    assert!(
                        !compact.contains("reqwest::Client"),
                        "direct worker HTTP client: {}",
                        path.display()
                    );
                }
            }
        }
        Ok(())
    }
    audit(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"))
}

#[test]
fn malformed_worker_credential_errors_are_redacted() -> TestResult {
    use videnoa_controller::{
        domain::{SecretString, WorkerApiUrl},
        remote::{PayloadLimits, RemoteTimeouts, VidenoaClient},
    };
    let credential = format!("{}\n", uuid::Uuid::new_v4());
    let result = VidenoaClient::new_with_password(
        WorkerApiUrl::parse("http://127.0.0.1:13000")?,
        RemoteTimeouts::new(
            std::time::Duration::from_secs(3),
            std::time::Duration::from_secs(3),
            std::time::Duration::from_secs(3),
        )?,
        PayloadLimits::new(4096, 1024)?,
        Some(&SecretString::new(&credential)),
    );
    let error = result.err().ok_or("invalid header accepted")?;
    assert!(!format!("{error:?} {error}").contains(credential.trim()));
    Ok(())
}
