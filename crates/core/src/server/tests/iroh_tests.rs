use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn iroh_password_and_api_lifecycle() -> anyhow::Result<()> {
    let state = test_state();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let url = format!("http://{address}");
    let app = app_router(state.clone());
    let http_server = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
    });
    let http = reqwest::Client::new();
    let mut config = state.inner.config.read().await.clone();
    config.iroh.enabled = true;
    assert_eq!(
        http.put(format!("{url}/api/config"))
            .json(&config)
            .send()
            .await?
            .status(),
        reqwest::StatusCode::BAD_REQUEST
    );
    // Deliberately synthetic credentials, isolated from production state.
    let password = "iroh test worker password";
    let created = http
        .put(format!("{url}/api/auth/password"))
        .header("Origin", &url)
        .json(&serde_json::json!({"password": password,"password_confirmation":password}))
        .send()
        .await?;
    assert!(created.status().is_success());
    let enabled = http
        .put(format!("{url}/api/config"))
        .bearer_auth(password)
        .json(&config)
        .send()
        .await?;
    assert!(enabled.status().is_success(), "{}", enabled.text().await?);
    let addr = state
        .iroh_addr()
        .await
        .ok_or_else(|| anyhow::anyhow!("endpoint missing"))?;
    let identity = addr.id;
    let client_root = tempfile::tempdir()?;
    let client = videnoa_transport::Client::open(client_root.path()).await?;
    assert!(client
        .tunnel(addr.clone(), "incorrect test password")
        .await
        .is_err());
    let mut stream = client.tunnel(addr.clone(), password).await?;
    stream.write_all(format!("GET /api/config HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {password}\r\nConnection: close\r\n\r\n").as_bytes()).await?;
    let mut response = Vec::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        stream.read_to_end(&mut response),
    )
    .await??;
    assert!(response.starts_with(b"HTTP/1.1 200"));
    // A retained raw tunnel remains usable after rotation; new streams need the new password.
    // Test-only in-memory job exercises WebSocket pushes without running inference.
    state.inner.jobs.insert(
        "iroh-socket-test".into(),
        Job {
            id: "iroh-socket-test".into(),
            status: JobStatus::Running,
            workflow: crate::graph::PipelineGraph::new(),
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            progress: None,
            error: None,
            cancel_token: tokio_util::sync::CancellationToken::new(),
            params: None,
            workflow_name: "iroh-socket-test".into(),
            workflow_source: "test".into(),
            rerun_of_job_id: None,
        },
    );
    let (sender, _) = tokio::sync::broadcast::channel(8);
    state
        .inner
        .progress_senders
        .insert("iroh-socket-test".into(), sender.clone());
    let mut websocket = client.tunnel(addr.clone(), password).await?;
    websocket.write_all(b"GET /api/jobs/iroh-socket-test/ws HTTP/1.1\r\nHost: localhost\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n").await?;
    let mut upgrade = Vec::new();
    while !upgrade.ends_with(b"\r\n\r\n") {
        upgrade.push(websocket.read_u8().await?);
    }
    assert!(upgrade.starts_with(b"HTTP/1.1 101"));
    let mut retained = client.tunnel(addr.clone(), password).await?;
    let next = "iroh rotated test password";
    assert!(http
        .put(format!("{url}/api/auth/password"))
        .bearer_auth(password)
        .json(&serde_json::json!({"password":next,"password_confirmation":next}))
        .send()
        .await?
        .status()
        .is_success());
    retained
        .write_all(b"GET /api/health HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await?;
    let mut head = [0; 12];
    retained.read_exact(&mut head).await?;
    assert_eq!(&head, b"HTTP/1.1 200");
    assert!(client.tunnel(addr.clone(), password).await.is_err());
    sender.send(JobWsEvent::Progress {
        current_frame: 1,
        total_frames: Some(2),
        fps: 1.0,
        eta_seconds: None,
    })?;
    let pushed = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        anyhow::ensure!(websocket.read_u8().await? == 0x81, "expected text frame");
        let length = websocket.read_u8().await?;
        anyhow::ensure!(length < 126, "expected small test frame");
        let mut body = vec![0; usize::from(length)];
        websocket.read_exact(&mut body).await?;
        anyhow::Ok(serde_json::from_slice::<serde_json::Value>(&body)?)
    })
    .await??;
    assert_eq!(pushed["current_frame"], 1);
    let mut active = client.tunnel(addr, next).await?;
    assert!(http
        .delete(format!("{url}/api/auth/password"))
        .bearer_auth(next)
        .send()
        .await?
        .status()
        .is_success());
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(5), active.read_u8())
            .await?
            .is_err()
    );
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(5), websocket.read_u8())
            .await?
            .is_err()
    );
    assert!(state.iroh_addr().await.is_none());
    assert!(
        !crate::config::AppConfig::load_from_path(&state.inner.config_path)?
            .iroh
            .enabled
    );
    let status: serde_json::Value = http
        .get(format!("{url}/api/iroh"))
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(status["endpoint_id"], identity.to_string());
    assert_eq!(status["running"], false);
    client.shutdown().await;
    state.shutdown_iroh().await;
    http_server.abort();
    Ok(())
}
