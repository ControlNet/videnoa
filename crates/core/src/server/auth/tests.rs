use super::*;

fn context(reply: Option<&Reply>) -> RequestContext {
    let mut headers = HeaderMap::new();
    headers.insert(header::HOST, "localhost:13000".parse().unwrap());
    headers.insert(header::ORIGIN, "http://localhost:13000".parse().unwrap());
    if let Some(reply) = reply {
        if let Some(cookie) = &reply.cookie {
            headers.insert(
                header::COOKIE,
                cookie.split(';').next().unwrap().parse().unwrap(),
            );
        }
        if let Some(csrf) = reply.body.get("csrf_token").and_then(Value::as_str) {
            headers.insert("x-csrf-token", csrf.parse().unwrap());
        }
    }
    RequestContext {
        headers,
        peer: Some("127.0.0.1".parse().unwrap()),
    }
}

fn setup(password: &str) -> Action {
    Action::SetPassword {
        password: password.to_owned(),
        confirmation: password.to_owned(),
    }
}

#[tokio::test]
async fn password_lifecycle_persists_and_revokes_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let service = AuthService::open(dir.path(), AuthConfig::default()).unwrap();
    assert!(!service
        .execute(context(None), Action::Session)
        .await
        .unwrap()
        .body["password_enabled"]
        .as_bool()
        .unwrap());
    let password = uuid::Uuid::new_v4().to_string();
    let first = service
        .execute(context(None), setup(&password))
        .await
        .unwrap();
    let first_context = context(Some(&first));
    assert!(service
        .execute(context(None), Action::Check { mutation: false })
        .await
        .is_err());
    assert!(service
        .execute(first_context.clone(), Action::Check { mutation: true })
        .await
        .is_ok());
    drop(service);
    let service = AuthService::open(dir.path(), AuthConfig::default()).unwrap();
    assert!(service
        .execute(first_context.clone(), Action::Check { mutation: false })
        .await
        .is_ok());
    let next = service
        .execute(first_context.clone(), setup("短"))
        .await
        .unwrap();
    assert!(service
        .execute(first_context, Action::Check { mutation: false })
        .await
        .is_err());
    assert!(service
        .execute(context(None), Action::Login { password })
        .await
        .is_err());
    service
        .execute(context(Some(&next)), Action::Disable)
        .await
        .unwrap();
    assert!(service
        .execute(context(None), Action::Check { mutation: true })
        .await
        .is_ok());
    drop(service);
    let service = AuthService::open(dir.path(), AuthConfig::default()).unwrap();
    assert_eq!(
        service
            .execute(context(None), Action::Session)
            .await
            .unwrap()
            .body["password_enabled"],
        false
    );
}

#[tokio::test]
async fn setup_race_csrf_and_explicit_bad_bearer_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let service = AuthService::open(dir.path(), AuthConfig::default()).unwrap();
    let (first, second) = tokio::join!(
        service.execute(context(None), setup("a")),
        service.execute(context(None), setup("b"))
    );
    assert_ne!(first.is_ok(), second.is_ok());
    let reply = first.or(second).unwrap();
    let mut authenticated = context(Some(&reply));
    authenticated.headers.remove("x-csrf-token");
    assert!(matches!(
        service.execute(authenticated, Action::Disable).await,
        Err(AuthError::Forbidden)
    ));
    let mut authenticated = context(Some(&reply));
    authenticated
        .headers
        .insert(header::AUTHORIZATION, "Bearer incorrect".parse().unwrap());
    assert!(matches!(
        service
            .execute(authenticated, Action::Check { mutation: false })
            .await,
        Err(AuthError::Unauthorized)
    ));
}

#[tokio::test]
async fn reset_requires_exclusive_access_and_preserves_other_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("config.toml"), "locale = 'en'").unwrap();
    let service = AuthService::open(dir.path(), AuthConfig::default()).unwrap();
    service.execute(context(None), setup("a")).await.unwrap();
    assert!(reset(dir.path()).is_err());
    drop(service);
    reset(dir.path()).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("config.toml")).unwrap(),
        "locale = 'en'"
    );
    let service = AuthService::open(dir.path(), AuthConfig::default()).unwrap();
    assert_eq!(
        service
            .execute(context(None), Action::Session)
            .await
            .unwrap()
            .body["password_enabled"],
        false
    );
}

#[tokio::test]
async fn reset_disables_persisted_iroh_without_replacing_identity() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = crate::config::AppConfig::default();
    config.iroh.enabled = true;
    config
        .save_to_path(&dir.path().join("config.toml"))
        .unwrap();
    let identity = videnoa_transport::Identity::open(dir.path()).unwrap();
    let endpoint_id = identity.id();
    drop(identity);
    let service = AuthService::open(dir.path(), AuthConfig::default()).unwrap();
    service.execute(context(None), setup("a")).await.unwrap();
    drop(service);
    reset(dir.path()).unwrap();
    let config = crate::config::AppConfig::load_from_path(&dir.path().join("config.toml")).unwrap();
    assert!(!config.iroh.enabled);
    assert_eq!(
        videnoa_transport::Identity::open(dir.path()).unwrap().id(),
        endpoint_id
    );
}

#[tokio::test]
async fn bearer_unicode_origin_limits_and_policy_changes() {
    let dir = tempfile::tempdir().unwrap();
    let service = AuthService::open(dir.path(), AuthConfig::default()).unwrap();
    let mut foreign = context(None);
    foreign
        .headers
        .insert(header::ORIGIN, "http://other.invalid".parse().unwrap());
    assert!(matches!(
        service.execute(foreign, setup("a")).await,
        Err(AuthError::Forbidden)
    ));
    for invalid in ["", "\n", &"a".repeat(1025)] {
        assert!(matches!(
            service.execute(context(None), setup(invalid)).await,
            Err(AuthError::Invalid)
        ));
    }
    let reply = service
        .execute(context(None), setup(" 中文 "))
        .await
        .unwrap();
    let mut bearer = context(None);
    bearer.headers.remove(header::ORIGIN);
    bearer.headers.insert(
        header::AUTHORIZATION,
        axum::http::HeaderValue::from_bytes("Bearer  中文 ".as_bytes()).unwrap(),
    );
    assert!(service
        .execute(bearer.clone(), Action::Check { mutation: true })
        .await
        .is_ok());
    service
        .reconfigure(AuthConfig {
            secure_cookie: true,
            ..AuthConfig::default()
        })
        .unwrap();
    assert!(matches!(
        service
            .execute(context(Some(&reply)), Action::Check { mutation: false })
            .await,
        Err(AuthError::Unauthorized)
    ));
    assert!(service
        .execute(bearer, Action::Check { mutation: true })
        .await
        .is_ok());
    let incorrect = uuid::Uuid::new_v4().to_string();
    for _ in 0..5 {
        assert!(matches!(
            service
                .execute(
                    context(None),
                    Action::Login {
                        password: incorrect.clone()
                    }
                )
                .await,
            Err(AuthError::Unauthorized)
        ));
    }
    assert!(matches!(
        service
            .execute(
                context(None),
                Action::Login {
                    password: incorrect.clone()
                }
            )
            .await,
        Err(AuthError::RateLimited)
    ));
}

#[tokio::test]
async fn shortening_expiry_is_immediate_and_extending_does_not_revive_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let service = AuthService::open(dir.path(), AuthConfig::default()).unwrap();
    let reply = service.execute(context(None), setup("a")).await.unwrap();
    let now = Utc::now().timestamp();
    service
        .0
        .database
        .lock()
        .unwrap()
        .connection
        .execute(
            "UPDATE sessions SET created=?, last_seen=?",
            params![now - 60, now - 10],
        )
        .unwrap();
    service
        .reconfigure(AuthConfig {
            session_absolute_seconds: 30,
            session_idle_seconds: 20,
            secure_cookie: false,
        })
        .unwrap();
    assert!(matches!(
        service
            .execute(context(Some(&reply)), Action::Check { mutation: false })
            .await,
        Err(AuthError::Unauthorized)
    ));
    service.reconfigure(AuthConfig::default()).unwrap();
    // A stored deadline remains authoritative even after a policy extension.
    service
        .0
        .database
        .lock()
        .unwrap()
        .connection
        .execute("UPDATE sessions SET expires=?", [now - 1])
        .unwrap();
    assert!(matches!(
        service
            .execute(context(Some(&reply)), Action::Check { mutation: false })
            .await,
        Err(AuthError::Unauthorized)
    ));
    assert!(AuthConfig {
        session_idle_seconds: 0,
        ..AuthConfig::default()
    }
    .validate()
    .is_err());
    assert!(AuthConfig {
        session_absolute_seconds: u64::MAX,
        ..AuthConfig::default()
    }
    .validate()
    .is_err());
}

#[tokio::test]
async fn http_routes_protect_business_apis_but_keep_health_and_websocket_public() {
    use axum::body::{to_bytes, Body};
    use tower::ServiceExt;
    let dir = tempfile::tempdir().unwrap();
    let config = crate::config::AppConfig::default();
    let state = super::super::app_state_with_config(
        config,
        dir.path().join("config.toml"),
        dir.path().to_owned(),
    );
    state.ensure_auth_ready().unwrap();
    let service = state.inner.auth.as_ref().unwrap();
    let password = uuid::Uuid::new_v4().to_string();
    let reply = service
        .execute(context(None), setup(&password))
        .await
        .unwrap();
    let router = super::super::app_router(state.clone());
    for endpoint in [
        "/api/about",
        "/api/jobs",
        "/api/config",
        "/api/nodes",
        "/api/fs/list",
        "/api/files/private",
        "/api/preview/frames/test/file.png",
    ] {
        let request = axum::http::Request::builder()
            .uri(endpoint)
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            router.clone().oneshot(request).await.unwrap().status(),
            StatusCode::UNAUTHORIZED,
            "{endpoint}"
        );
    }
    let response = router
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    // Missing job/upgrade metadata can fail normally, but must never require authentication.
    let response = router
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/jobs/missing/ws")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_ne!(response.status(), StatusCode::UNAUTHORIZED);
    let mut request = axum::http::Request::builder()
        .uri("/api/config")
        .body(Body::empty())
        .unwrap();
    *request.headers_mut() = context(Some(&reply)).headers;
    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert!(!std::str::from_utf8(&body).unwrap().contains(&password));
    assert!(!std::str::from_utf8(&body).unwrap().contains("argon2"));
    let mut request = axum::http::Request::builder()
        .uri("/api/jobs")
        .header(header::AUTHORIZATION, format!("Bearer {password}"))
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(ConnectInfo(
        "127.0.0.1:13000".parse::<SocketAddr>().unwrap(),
    ));
    assert_eq!(
        router.oneshot(request).await.unwrap().status(),
        StatusCode::OK
    );
}

#[test]
fn corrupt_authentication_database_does_not_open() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("auth.sqlite3"), b"corrupt database").unwrap();
    assert!(AuthService::open(dir.path(), AuthConfig::default()).is_err());
}

#[tokio::test]
async fn anonymous_websocket_survives_password_rotation_and_disable() {
    use super::super::{Job, JobStatus, JobWsEvent};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let dir = tempfile::tempdir().unwrap();
    let state = super::super::app_state_with_config(
        crate::config::AppConfig::default(),
        dir.path().join("config.toml"),
        dir.path().to_owned(),
    );
    let auth = state.inner.auth.as_ref().unwrap().clone();
    let session = auth.execute(context(None), setup("a")).await.unwrap();
    // Test-only in-memory job: exercises the real socket without starting inference.
    state.inner.jobs.insert(
        "socket-test".into(),
        Job {
            id: "socket-test".into(),
            status: JobStatus::Running,
            workflow: crate::graph::PipelineGraph::new(),
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            progress: None,
            error: None,
            cancel_token: tokio_util::sync::CancellationToken::new(),
            params: None,
            workflow_name: "socket-test".into(),
            workflow_source: "test".into(),
            rerun_of_job_id: None,
        },
    );
    let (sender, _) = tokio::sync::broadcast::channel(8);
    state
        .inner
        .progress_senders
        .insert("socket-test".into(), sender.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = super::super::app_router(state);
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    stream.write_all(format!("GET /api/jobs/socket-test/ws HTTP/1.1\r\nHost: {address}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n").as_bytes()).await.unwrap();
    let mut headers = Vec::new();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while !headers.ends_with(b"\r\n\r\n") {
            headers.push(stream.read_u8().await.unwrap());
        }
    })
    .await
    .unwrap();
    assert!(headers.starts_with(b"HTTP/1.1 101"));
    let rotated = auth
        .execute(context(Some(&session)), setup("b"))
        .await
        .unwrap();
    for frame in 1..=2 {
        if frame == 2 {
            auth.execute(context(Some(&rotated)), Action::Disable)
                .await
                .unwrap();
        }
        sender
            .send(JobWsEvent::Progress {
                current_frame: frame,
                total_frames: Some(2),
                fps: 1.0,
                eta_seconds: None,
            })
            .unwrap();
        let value = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            assert_eq!(stream.read_u8().await.unwrap(), 0x81);
            let length = stream.read_u8().await.unwrap();
            assert!(length < 126);
            let mut body = vec![0; length as usize];
            stream.read_exact(&mut body).await.unwrap();
            serde_json::from_slice::<Value>(&body).unwrap()
        })
        .await
        .unwrap();
        assert_eq!(value["current_frame"], frame);
    }
    drop(stream);
    server.abort();
}

fn bearer_context(password: &str) -> RequestContext {
    let mut request = context(None);
    request.headers.insert(
        header::AUTHORIZATION,
        axum::http::HeaderValue::from_bytes(format!("Bearer {password}").as_bytes()).unwrap(),
    );
    request
}

#[tokio::test]
async fn bearer_cache_is_ephemeral_and_tracks_committed_password_rotation() {
    let dir = tempfile::tempdir().unwrap();
    let first = uuid::Uuid::new_v4().to_string();
    let second = format!(" 短{} ", uuid::Uuid::new_v4());
    let service = AuthService::open(dir.path(), AuthConfig::default()).unwrap();
    service.execute(context(None), setup(&first)).await.unwrap();
    assert!(service
        .0
        .database
        .lock()
        .unwrap()
        .cached_password_digest
        .is_some());
    drop(service);

    let service = AuthService::open(dir.path(), AuthConfig::default()).unwrap();
    assert!(service
        .0
        .database
        .lock()
        .unwrap()
        .cached_password_digest
        .is_none());
    for expected in [1, 1, 1] {
        service
            .execute(bearer_context(&first), Action::Check { mutation: false })
            .await
            .unwrap();
        assert_eq!(
            service.0.database.lock().unwrap().slow_verifications,
            expected
        );
    }
    assert!(matches!(
        service
            .execute(bearer_context(&second), Action::Check { mutation: false })
            .await,
        Err(AuthError::Unauthorized)
    ));
    let rotated = service
        .execute(bearer_context(&first), setup(&second))
        .await
        .unwrap();
    let before = service.0.database.lock().unwrap().slow_verifications;
    service
        .execute(bearer_context(&second), Action::Check { mutation: true })
        .await
        .unwrap();
    assert_eq!(
        service.0.database.lock().unwrap().slow_verifications,
        before
    );
    assert!(matches!(
        service
            .execute(bearer_context(&first), Action::Check { mutation: false })
            .await,
        Err(AuthError::Unauthorized)
    ));
    {
        let database = service.0.database.lock().unwrap();
        let hash = stored_hash(&database.connection).unwrap().unwrap();
        assert!(hash.starts_with("$argon2id$"));
        assert!(Argon2::default()
            .verify_password(second.as_bytes(), &PasswordHash::new(&hash).unwrap())
            .is_ok());
        let columns: Vec<String> = database
            .connection
            .prepare("PRAGMA table_info(credential)")
            .unwrap()
            .query_map([], |row| row.get(1))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(columns, ["id", "password_hash"]);
    }
    service
        .execute(context(Some(&rotated)), Action::Disable)
        .await
        .unwrap();
    assert!(service
        .0
        .database
        .lock()
        .unwrap()
        .cached_password_digest
        .is_none());
    service.execute(context(None), setup(&first)).await.unwrap();
    assert!(reset(dir.path()).is_err()); // A live instance cannot retain a cache across external reset.
    drop(service);
    reset(dir.path()).unwrap();
    let service = AuthService::open(dir.path(), AuthConfig::default()).unwrap();
    let database = service.0.database.lock().unwrap();
    assert!(database.cached_password_digest.is_none());
    assert!(stored_hash(&database.connection).unwrap().is_none());
}
