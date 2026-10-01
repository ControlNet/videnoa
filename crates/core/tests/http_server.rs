use std::net::{Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocketUpgrade};
use axum::extract::ConnectInfo;
use axum::routing::{get, post};
use axum::Router;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{oneshot, Notify};
use videnoa_core::server::http_server::{HttpServer, HEADER_READ_TIMEOUT};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

const TEST_HEADER_TIMEOUT: Duration = Duration::from_millis(200);
const CLOSE_BOUND: Duration = Duration::from_secs(2);

async fn bind() -> TestResult<(TcpListener, SocketAddr)> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    Ok((listener, address))
}

async fn read_until_closed(stream: &mut TcpStream) -> TestResult<Vec<u8>> {
    let mut received = Vec::new();
    match tokio::time::timeout(CLOSE_BOUND, stream.read_to_end(&mut received)).await {
        Ok(Ok(_)) => Ok(received),
        // A reset also proves the server dropped the socket.
        Ok(Err(error)) if error.kind() == std::io::ErrorKind::ConnectionReset => Ok(received),
        Ok(Err(error)) => Err(error.into()),
        Err(_) => Err("server kept the connection open".into()),
    }
}

async fn request(stream: &mut TcpStream, head: &str) -> TestResult<String> {
    stream.write_all(head.as_bytes()).await?;
    let response = read_until_closed(stream).await?;
    Ok(String::from_utf8(response)?)
}

#[test]
fn production_header_read_timeout_is_thirty_seconds() {
    assert_eq!(HEADER_READ_TIMEOUT, Duration::from_secs(30));
}

#[tokio::test]
async fn incomplete_request_head_is_closed_after_header_timeout() -> TestResult {
    // Given: a server whose request-head deadline is short.
    let (listener, address) = bind().await?;
    let server = tokio::spawn(
        HttpServer::new(listener, Router::new().route("/", get(|| async { "ok" })))
            .header_read_timeout(TEST_HEADER_TIMEOUT)
            .serve(),
    );

    // When: a peer sends a request head that never terminates.
    let mut stream = TcpStream::connect(address).await?;
    stream.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n").await?;
    let started = Instant::now();

    // Then: the server closes the socket instead of waiting forever.
    let received = read_until_closed(&mut stream).await?;
    server.abort();
    assert!(started.elapsed() >= TEST_HEADER_TIMEOUT / 2);
    assert!(!String::from_utf8_lossy(&received).starts_with("HTTP/1.1 200"));
    Ok(())
}

#[tokio::test]
async fn complete_request_is_served_with_peer_connect_info() -> TestResult {
    // Given: a route that echoes the peer address from ConnectInfo.
    let (listener, address) = bind().await?;
    let router = Router::new().route(
        "/peer",
        get(|ConnectInfo(peer): ConnectInfo<SocketAddr>| async move { peer.to_string() }),
    );
    let server = tokio::spawn(
        HttpServer::new(listener, router)
            .header_read_timeout(TEST_HEADER_TIMEOUT)
            .serve(),
    );

    // When: a peer sends a complete request.
    let mut stream = TcpStream::connect(address).await?;
    let peer = stream.local_addr()?;
    let response = request(
        &mut stream,
        "GET /peer HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    )
    .await?;
    server.abort();

    // Then: it is answered and the handler saw the real peer address.
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.ends_with(&peer.to_string()), "{response}");
    Ok(())
}

#[tokio::test]
async fn header_timeout_does_not_bound_slow_bodies_or_handlers() -> TestResult {
    // Given: a handler that outlives the header deadline before responding.
    let (listener, address) = bind().await?;
    let router = Router::new().route(
        "/upload",
        post(|body: String| async move {
            tokio::time::sleep(TEST_HEADER_TIMEOUT * 3).await;
            body
        }),
    );
    let server = tokio::spawn(
        HttpServer::new(listener, router)
            .header_read_timeout(TEST_HEADER_TIMEOUT)
            .serve(),
    );

    // When: the head arrives promptly but the body trickles in after the deadline.
    let mut stream = TcpStream::connect(address).await?;
    stream
        .write_all(
            b"POST /upload HTTP/1.1\r\nHost: x\r\nConnection: close\r\nContent-Length: 4\r\n\r\nab",
        )
        .await?;
    tokio::time::sleep(TEST_HEADER_TIMEOUT * 3).await;
    let response = request(&mut stream, "cd").await?;
    server.abort();

    // Then: only the request head was bounded.
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.ends_with("abcd"), "{response}");
    Ok(())
}

#[tokio::test]
async fn graceful_shutdown_stops_accepting_and_drains_in_flight_requests() -> TestResult {
    // Given: one in-flight request parked in its handler and one idle connection.
    let (listener, address) = bind().await?;
    let entered = std::sync::Arc::new(Notify::new());
    let release = std::sync::Arc::new(Notify::new());
    let router = Router::new().route(
        "/slow",
        get({
            let entered = entered.clone();
            let release = release.clone();
            move || async move {
                entered.notify_one();
                release.notified().await;
                "drained"
            }
        }),
    );
    let (signal, stop) = oneshot::channel::<()>();
    let mut server = tokio::spawn(
        HttpServer::new(listener, router)
            .header_read_timeout(TEST_HEADER_TIMEOUT)
            .serve_with_graceful_shutdown(async move {
                let _ = stop.await;
            }),
    );
    let mut in_flight = TcpStream::connect(address).await?;
    in_flight
        .write_all(b"GET /slow HTTP/1.1\r\nHost: x\r\n\r\n")
        .await?;
    tokio::time::timeout(CLOSE_BOUND, entered.notified()).await?;
    let mut idle = TcpStream::connect(address).await?;

    // When: shutdown is signalled.
    signal.send(()).map_err(|()| "server dropped the signal")?;

    // Then: new connections are refused, the idle one closes, and the server waits.
    let deadline = Instant::now() + CLOSE_BOUND;
    while TcpStream::connect(address).await.is_ok() {
        assert!(Instant::now() < deadline, "listener still accepting");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    read_until_closed(&mut idle).await?;
    assert!(tokio::time::timeout(TEST_HEADER_TIMEOUT, &mut server)
        .await
        .is_err());

    // Then: the in-flight request completes and the server returns.
    release.notify_one();
    let response = String::from_utf8(read_until_closed(&mut in_flight).await?)?;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.ends_with("drained"), "{response}");
    tokio::time::timeout(CLOSE_BOUND, server).await???;
    Ok(())
}

#[tokio::test]
async fn websocket_upgrade_outlives_header_timeout() -> TestResult {
    // Given: an echo WebSocket route, like the Worker's job progress socket.
    let (listener, address) = bind().await?;
    let router = Router::new().route(
        "/ws",
        get(|upgrade: WebSocketUpgrade| async move {
            upgrade.on_upgrade(|mut socket| async move {
                while let Some(Ok(message)) = socket.recv().await {
                    if let Message::Text(text) = message {
                        if socket.send(Message::Text(text)).await.is_err() {
                            break;
                        }
                    }
                }
            })
        }),
    );
    let server = tokio::spawn(
        HttpServer::new(listener, router)
            .header_read_timeout(TEST_HEADER_TIMEOUT)
            .serve(),
    );

    // When: a client upgrades and stays quiet for longer than the header deadline.
    let mut stream = TcpStream::connect(address).await?;
    stream
        .write_all(
            b"GET /ws HTTP/1.1\r\nHost: x\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\
              Sec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n",
        )
        .await?;
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        let mut byte = [0_u8; 1];
        tokio::time::timeout(CLOSE_BOUND, stream.read_exact(&mut byte)).await??;
        head.push(byte[0]);
    }
    tokio::time::sleep(TEST_HEADER_TIMEOUT * 3).await;
    let mask = [1_u8, 2, 3, 4];
    let mut frame = vec![0x81, 0x80 | 4];
    frame.extend_from_slice(&mask);
    frame.extend(b"ping".iter().zip(mask.iter().cycle()).map(|(b, m)| b ^ m));
    stream.write_all(&frame).await?;
    let mut echoed = [0_u8; 6];
    tokio::time::timeout(CLOSE_BOUND, stream.read_exact(&mut echoed)).await??;
    server.abort();

    // Then: the upgrade succeeded and the socket was not cut by the head deadline.
    let head = String::from_utf8(head)?;
    assert!(head.starts_with("HTTP/1.1 101"), "{head}");
    assert_eq!(&echoed, b"\x81\x04ping");
    Ok(())
}

#[tokio::test]
async fn http2_prior_knowledge_is_not_served() -> TestResult {
    // Given: a server; no client of ours speaks cleartext HTTP/2 (h2c).
    let (listener, address) = bind().await?;
    let server = tokio::spawn(
        HttpServer::new(listener, Router::new().route("/", get(|| async { "ok" })))
            .header_read_timeout(TEST_HEADER_TIMEOUT)
            .serve(),
    );

    // When: a peer opens with the HTTP/2 connection preface.
    let mut stream = TcpStream::connect(address).await?;
    stream
        .write_all(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n")
        .await?;

    // Then: the server never answers with an HTTP/2 SETTINGS frame (frame
    // type 0x4 at byte 3), so h2c cannot bypass the request-head deadline.
    let received = read_until_closed(&mut stream).await?;
    server.abort();
    assert!(
        received.len() < 4 || received[3] != 0x4,
        "server spoke HTTP/2: {received:02x?}"
    );
    Ok(())
}
