#![cfg(unix)]

use std::error::Error;
use std::fs;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

use axum::http::header;
use futures_util::StreamExt;
use tempfile::TempDir;
use tokio::net::TcpStream;
use tokio::process::{Child, Command};
use videnoa_controller::auth::hash_password;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

const PROCESS_EXIT_BOUND: Duration = Duration::from_secs(30);
const STARTUP_BOUND: Duration = Duration::from_secs(10);

struct ProcessFixture {
    _directory: TempDir,
    address: SocketAddr,
    config_path: PathBuf,
    password: String,
}

impl ProcessFixture {
    fn new() -> TestResult<Self> {
        let directory = TempDir::new()?;
        let input_root = directory.path().join("input");
        let output_root = directory.path().join("output");
        let data_root = directory.path().join("data");
        let temp_root = directory.path().join("temp");
        for root in [&input_root, &output_root, &data_root, &temp_root] {
            fs::create_dir(root)?;
        }
        let password = uuid::Uuid::new_v4().to_string();
        let hash_path = directory.path().join("admin-password.phc");
        fs::write(&hash_path, hash_password(&password)?)?;
        let reservation = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let address = reservation.local_addr()?;
        drop(reservation);
        let config_path = directory.path().join("controller.toml");
        fs::write(
            &config_path,
            format!(
                "[server]\nhost = \"127.0.0.1\"\nport = {}\n\n[paths]\ninput_roots = [\"{}\"]\noutput_roots = [\"{}\"]\ndata_root = \"{}\"\ntemp_root = \"{}\"\n\n[auth]\npassword_hash_file = \"{}\"\nsecure_cookie = false\nsession_absolute_seconds = 86400\nsession_idle_seconds = 3600\n",
                address.port(),
                input_root.display(),
                output_root.display(),
                data_root.display(),
                temp_root.display(),
                hash_path.display(),
            ),
        )?;
        Ok(Self {
            _directory: directory,
            address,
            config_path,
            password,
        })
    }

    fn spawn(&self) -> TestResult<Child> {
        let mut command = Command::new(env!("CARGO_BIN_EXE_videnoa-controller"));
        command
            .arg("--config")
            .arg(&self.config_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        Ok(command.spawn()?)
    }
}

#[tokio::test]
async fn sigterm_with_authenticated_sse_exits_inside_drain_bound() -> TestResult {
    // Given: a real Controller child with an authenticated cookie SSE response kept alive.
    let fixture = ProcessFixture::new()?;
    let mut child = fixture.spawn()?;

    // When: SIGTERM starts graceful shutdown without the SSE client disconnecting.
    let result = exercise_shutdown(&fixture, &mut child).await;

    // Then: the child exits inside the drain bound, and failures are reaped before cleanup.
    if result.is_err() {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    result
}

async fn exercise_shutdown(fixture: &ProcessFixture, child: &mut Child) -> TestResult {
    wait_for_listener(fixture.address, child).await?;
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .build()?;
    let base_url = format!("http://{}", fixture.address);
    let login = client
        .post(format!("{base_url}/api/auth/login"))
        .json(&serde_json::json!({"password": fixture.password}))
        .send()
        .await?;
    if !login.status().is_success() {
        return Err(std::io::Error::other("Controller login failed").into());
    }
    let cookie = login
        .headers()
        .get(header::SET_COOKIE)
        .ok_or_else(|| std::io::Error::other("Controller login omitted session cookie"))?
        .to_str()?
        .split(';')
        .next()
        .ok_or_else(|| std::io::Error::other("Controller session cookie is empty"))?
        .to_owned();
    let events = client
        .get(format!("{base_url}/api/events"))
        .header(header::ACCEPT, "text/event-stream")
        .header(header::COOKIE, cookie)
        .send()
        .await?
        .error_for_status()?;
    let mut event_stream = events.bytes_stream();
    wait_for_initial_refetch(&mut event_stream).await?;

    let started = Instant::now();
    let signal = Command::new("kill")
        .arg("-TERM")
        .arg(
            child
                .id()
                .ok_or_else(|| std::io::Error::other("child pid missing"))?
                .to_string(),
        )
        .status()
        .await?;
    if !signal.success() {
        return Err(std::io::Error::other("failed to send SIGTERM").into());
    }
    let listener_closed = wait_for_listener_closure(fixture.address).await?;
    let exit = tokio::time::timeout(PROCESS_EXIT_BOUND, child.wait())
        .await
        .map_err(|_| {
            std::io::Error::other(format!(
                "Controller listener closed in {} ms but the process remained alive after the 30-second shutdown bound",
                listener_closed.as_millis()
            ))
        })??;
    let process_exit = started.elapsed();
    if !exit.success() {
        return Err(
            std::io::Error::other(format!("Controller exited unsuccessfully: {exit}")).into(),
        );
    }
    eprintln!(
        "sse_shutdown_process listener_closed_ms={} process_exit_ms={}",
        listener_closed.as_millis(),
        process_exit.as_millis()
    );
    drop(event_stream);
    Ok(())
}

async fn wait_for_listener(address: SocketAddr, child: &mut Child) -> TestResult {
    tokio::time::timeout(STARTUP_BOUND, async {
        loop {
            if let Ok(stream) = TcpStream::connect(address).await {
                drop(stream);
                return Ok(());
            }
            if let Some(status) = child.try_wait()? {
                return Err(std::io::Error::other(format!(
                    "Controller exited before accepting connections: {status}"
                )));
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| std::io::Error::other("Controller listener did not open"))??;
    Ok(())
}

async fn wait_for_initial_refetch<S>(stream: &mut S) -> TestResult
where
    S: futures_util::Stream<Item = Result<axum::body::Bytes, reqwest::Error>> + Unpin,
{
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut received = Vec::new();
        loop {
            let chunk = stream
                .next()
                .await
                .ok_or_else(|| std::io::Error::other("SSE closed before initial refetch"))??;
            received.extend_from_slice(&chunk);
            if received
                .windows(b"event: refetch".len())
                .any(|window| window == b"event: refetch")
            {
                return TestResult::Ok(());
            }
        }
    })
    .await
    .map_err(|_| std::io::Error::other("SSE omitted initial refetch"))??;
    Ok(())
}

async fn wait_for_listener_closure(address: SocketAddr) -> TestResult<Duration> {
    let started = Instant::now();
    Ok(tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match TcpStream::connect(address).await {
                Ok(stream) => drop(stream),
                Err(_) => return started.elapsed(),
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| std::io::Error::other("Controller listener remained open after SIGTERM"))?)
}
