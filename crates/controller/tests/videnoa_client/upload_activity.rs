//! Real TCP tests with synthetic bytes, paced readers, and explicit mock checkpoints.
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::time::{sleep, timeout, Instant};

use super::{test_client, MockVidenoa, TestResult, JSON_LIMIT};
use crate::mock_videnoa::checkpoints::Checkpoint;
use videnoa_controller::domain::WorkerApiUrl;
use videnoa_controller::remote::{
    FileApiPath, PayloadLimits, RemoteTimeouts, VidenoaClient, VidenoaClientError,
};

const STALL: Duration = Duration::from_millis(250);
const STEP: Duration = Duration::from_millis(50);
const CHUNKS: usize = 60;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn active_upload_has_no_total_transfer_deadline() -> TestResult {
    let server = MockVidenoa::start().await?;
    let client = test_client(&server, Duration::from_millis(50), STALL, JSON_LIMIT)?;
    let path = FileApiPath::parse("activity/input.bin")?;
    let (mut source, reader) = tokio::io::duplex(1024);
    let producer = tokio::spawn(async move {
        for _ in 0..CHUNKS {
            source.write_all(&[7_u8; 1024]).await?;
            sleep(STEP).await;
        }
        Ok::<_, std::io::Error>(())
    });
    let started = Instant::now();
    let receipt = client.upload(&path, (CHUNKS * 1024) as u64, reader).await?;
    assert_eq!(receipt.size, (CHUNKS * 1024) as u64);
    assert!(started.elapsed() >= STALL * 10);
    producer.await??;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mid_upload_reader_inactivity_is_stall() -> TestResult {
    let server = MockVidenoa::start().await?;
    let client = test_client(&server, Duration::from_secs(5), STALL, JSON_LIMIT)?;
    let path = FileApiPath::parse("activity/stalled.bin")?;
    let (mut source, reader) = tokio::io::duplex(1024);
    let uploading = tokio::spawn(async move { client.upload(&path, 8192, reader).await });
    for _ in 0..3 {
        source.write_all(&[8_u8; 1024]).await?;
        sleep(STEP).await;
    }
    // Keep the reader open without EOF or further bytes.
    assert_eq!(
        timeout(Duration::from_secs(5), uploading).await??,
        Err(VidenoaClientError::Stall)
    );
    drop(source);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn network_backpressure_eventually_stalls_upload_body() -> TestResult {
    let server = MockVidenoa::start().await?;
    let client = test_client(&server, Duration::from_secs(5), STALL, JSON_LIMIT)?;
    let path = FileApiPath::parse("activity/backpressure.bin")?;
    let ticket = server.pause(Checkpoint::BeforeAcceptingUpload).await;
    let uploading = tokio::spawn(async move {
        let size = 512 * 1024 * 1024;
        client
            .upload(&path, size, tokio::io::repeat(9).take(size))
            .await
    });
    server.await_checkpoint(&ticket).await?;
    let result = timeout(Duration::from_secs(10), uploading).await??;
    server.release(ticket).await?;
    assert_eq!(result, Err(VidenoaClientError::Stall));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn active_download_has_no_total_transfer_deadline() -> TestResult {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let client = VidenoaClient::new(
        WorkerApiUrl::parse(&format!("http://{}", listener.local_addr()?))?,
        RemoteTimeouts::new(Duration::from_secs(2), Duration::from_millis(50), STALL)?,
        PayloadLimits::new(JSON_LIMIT, 1024)?,
    )?;
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await?;
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            request.push(socket.read_u8().await?);
        }
        socket
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    CHUNKS * 1024
                )
                .as_bytes(),
            )
            .await?;
        for _ in 0..CHUNKS {
            socket.write_all(&[6_u8; 1024]).await?;
            sleep(STEP).await;
        }
        Ok::<_, std::io::Error>(())
    });
    let started = Instant::now();
    let mut bytes = Vec::new();
    client
        .download(&FileApiPath::parse("active.bin")?, &mut bytes)
        .await?;
    assert_eq!(bytes, vec![6_u8; CHUNKS * 1024]);
    assert!(started.elapsed() >= STALL * 10);
    server.await??;
    Ok(())
}

#[tokio::test]
async fn local_upload_read_failure_remains_local_io() -> TestResult {
    let server = MockVidenoa::start().await?;
    let client = test_client(&server, Duration::from_secs(5), STALL, JSON_LIMIT)?;
    let reader = tokio_util::io::StreamReader::new(futures_util::stream::iter([
        Ok(axum::body::Bytes::from_static(b"test-only bytes")),
        Err(std::io::Error::other("test-only read failure")),
    ]));
    assert_eq!(
        client
            .upload(&FileApiPath::parse("broken.bin")?, 100, reader)
            .await,
        Err(VidenoaClientError::LocalIo)
    );
    Ok(())
}
