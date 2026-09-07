use chrono::Utc;
use serde_json::json;
use tempfile::TempDir;
use videnoa_controller::domain::{WorkerCreateRequest, WorkerUpdateRequest};
use videnoa_controller::persistence::{Database, DatabaseOptions, Store};
use videnoa_controller::workers::WorkerRegistry;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
async fn native_endpoint_registration_preserves_identity_and_password_contract() -> TestResult {
    let root = TempDir::new()?;
    let identity_root = TempDir::new()?;
    let id = videnoa_transport::Identity::open(identity_root.path())?.id();
    let path = root.path().join("controller.sqlite3");
    let database = Database::open(DatabaseOptions::new(&path)).await?;
    let registry = WorkerRegistry::new(Store::new(database.clone()));
    // Password is synthetic test data; no production credentials are used.
    let request = json!({"transport":"iroh","endpoint_id":id.to_string(),"password":"test worker credential","name":"iroh-worker","enabled":true,"compute_slots":1});
    let parsed: WorkerCreateRequest = serde_json::from_value(request.clone())?;
    let wire = serde_json::to_value(&parsed)?;
    assert_eq!(wire["endpoint_id"], id.to_string());
    assert!(wire.get("api_url").is_none());
    let worker = registry.create(parsed, Utc::now()).await?;
    assert_eq!(worker.api_url.iroh_id(), Some(id));
    let stored: (String, String) =
        sqlx::query_as("SELECT transport, endpoint_id FROM workers WHERE id = ?")
            .bind(worker.id.to_string())
            .fetch_one(database.pool())
            .await?;
    assert_eq!(stored, ("iroh".into(), id.to_string()));
    let mut duplicate = request.clone();
    duplicate["name"] = json!("another-name");
    assert!(registry
        .create(serde_json::from_value(duplicate)?, Utc::now())
        .await
        .is_err());
    let mut clear = request.clone();
    clear["version"] = json!(worker.version);
    clear["password"] = serde_json::Value::Null;
    assert!(registry
        .update(
            worker.id,
            serde_json::from_value::<WorkerUpdateRequest>(clear)?,
            Utc::now()
        )
        .await
        .is_err());
    drop(registry);
    database.close().await;
    let reopened = Database::open(DatabaseOptions::new(&path)).await?;
    let restored = Store::new(reopened)
        .worker(worker.id)
        .await?
        .ok_or("worker missing")?;
    assert_eq!(restored.api_url.iroh_id(), Some(id));
    Ok(())
}

#[test]
fn endpoint_wire_contract_rejects_mixed_fields_and_tickets() {
    for endpoint in [
        json!({"transport":"iroh","endpoint_id":"not-an-endpoint-ticket"}),
        json!({"transport":"iroh","api_url":"http://localhost:3000"}),
        json!({"api_url":"iroh://invalid/"}),
        json!({"transport":"http","endpoint_id":"invalid","api_url":"http://localhost:3000"}),
    ] {
        let mut request = json!({"name":"worker","enabled":true,"compute_slots":1});
        request
            .as_object_mut()
            .unwrap()
            .extend(endpoint.as_object().unwrap().clone());
        assert!(serde_json::from_value::<WorkerCreateRequest>(request).is_err());
    }
}
