use std::io::Write as _;
use std::net::{Ipv4Addr, Shutdown, SocketAddr};

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tower::ServiceExt;

use super::support::{fixture, TestResult, PASSWORD};

struct SseStream {
    response: reqwest::Response,
    buffered: Vec<u8>,
}

impl SseStream {
    const fn new(response: reqwest::Response) -> Self {
        Self {
            response,
            buffered: Vec::new(),
        }
    }

    async fn next_event(&mut self) -> TestResult<(String, Value)> {
        loop {
            if let Some(end) = self
                .buffered
                .windows(2)
                .position(|window| window == b"\n\n")
            {
                let frame = self.buffered.drain(..end + 2).collect::<Vec<_>>();
                let frame = std::str::from_utf8(&frame)?;
                let event = frame
                    .lines()
                    .find_map(|line| line.strip_prefix("event: "))
                    .ok_or("SSE event name missing")?;
                let data = frame
                    .lines()
                    .find_map(|line| line.strip_prefix("data: "))
                    .ok_or("SSE event data missing")?;
                return Ok((event.to_owned(), serde_json::from_str(data)?));
            }
            let chunk = self.response.chunk().await?.ok_or("SSE stream closed")?;
            self.buffered.extend_from_slice(&chunk);
        }
    }
}

async fn consume_initial_refetch(stream: &mut TcpStream) -> TestResult {
    let mut received = Vec::new();
    let mut chunk = [0_u8; 1024];
    while !received
        .windows(b"event: refetch".len())
        .any(|window| window == b"event: refetch")
    {
        let count = stream.read(&mut chunk).await?;
        if count == 0 {
            return Err(std::io::Error::other("SSE connection closed before refetch").into());
        }
        received.extend_from_slice(&chunk[..count]);
    }
    Ok(())
}

#[tokio::test]
async fn authenticated_sse_disconnect_reconnect_refetches_without_history_replay() -> TestResult {
    // Given: a real authenticated TCP stream that has consumed its initial snapshot signal.
    let fixture = fixture().await?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let (shutdown, shutdown_receiver) = oneshot::channel();
    let router = fixture.router.clone();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            let _ = shutdown_receiver.await;
        })
        .await
    });
    let events_url = format!("http://{address}/api/events");
    let workers_url = format!("http://{address}/api/workers");
    let mut first_stream = TcpStream::connect(address).await?;
    first_stream
        .write_all(
            format!(
                "GET /api/events HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {PASSWORD}\r\nAccept: text/event-stream\r\n\r\n"
            )
            .as_bytes(),
        )
        .await?;
    consume_initial_refetch(&mut first_stream).await?;

    // When: the first transport is dropped, a historical update commits, and a new connection opens.
    let mut disconnected = first_stream.into_std()?;
    disconnected.shutdown(Shutdown::Both)?;
    assert!(disconnected.write_all(b"\r\n").is_err());
    drop(disconnected);
    let client = reqwest::Client::new();
    let created = client
        .post(&workers_url)
        .bearer_auth(PASSWORD)
        .json(&json!({
            "name": "historical-worker",
            "api_url": "https://worker.example/api/",
            "enabled": true,
            "compute_slots": 1
        }))
        .send()
        .await?;
    assert_eq!(created.status(), StatusCode::CREATED);
    let worker: Value = created.json().await?;
    let worker_id = worker["id"].as_str().ok_or("worker id missing")?;
    let second_response = client.get(&events_url).bearer_auth(PASSWORD).send().await?;
    assert_eq!(second_response.status(), StatusCode::OK);
    let mut second_stream = SseStream::new(second_response);
    assert_eq!(second_stream.next_event().await?.0, "refetch");
    let update_url = format!("{workers_url}/{worker_id}");
    let updated = client
        .put(update_url)
        .bearer_auth(PASSWORD)
        .json(&json!({
            "version": 0,
            "name": "current-worker",
            "api_url": "https://worker.example/api/",
            "enabled": true,
            "compute_slots": 1
        }))
        .send()
        .await?;
    assert_eq!(updated.status(), StatusCode::OK);

    // Then: the next frame is only the current durable update, never the disconnected history.
    let (event, data) = second_stream.next_event().await?;
    assert_eq!(event, "worker_updated");
    assert_eq!(data["data"]["worker"]["name"], "current-worker");
    assert_eq!(data["data"]["worker"]["version"], 1);
    drop(second_stream);
    shutdown
        .send(())
        .map_err(|()| std::io::Error::other("SSE server shutdown receiver closed"))?;
    server.await??;
    Ok(())
}

#[tokio::test]
async fn cross_origin_preflight_never_receives_cors_authorization() -> TestResult {
    // Given: an attacker-origin preflight for an authenticated mutation route.
    let fixture = fixture().await?;
    let mut request = Request::builder()
        .method("OPTIONS")
        .uri("/api/tasks")
        .header(header::ORIGIN, "https://attacker.invalid")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
        .body(Body::empty())?;
    super::support::add_peer(&mut request);

    // When: the complete production router evaluates the preflight.
    let response = fixture.router.oneshot(request).await?;

    // Then: no CORS policy grants the origin or credentials.
    assert!(matches!(
        response.status(),
        StatusCode::UNAUTHORIZED | StatusCode::METHOD_NOT_ALLOWED
    ));
    assert!(!response
        .headers()
        .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN));
    assert!(!response
        .headers()
        .contains_key(header::ACCESS_CONTROL_ALLOW_CREDENTIALS));
    eprintln!("task21_security cors_origin_header=absent cors_credentials_header=absent");
    Ok(())
}
