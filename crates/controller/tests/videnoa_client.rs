use std::collections::BTreeMap;
use std::error::Error;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

#[path = "mock_videnoa/faults.rs"]
mod harness_faults;
#[path = "mock_videnoa/happy.rs"]
mod harness_happy;
#[path = "mock_videnoa/idempotency.rs"]
mod harness_idempotency;
#[path = "mock_videnoa/restart.rs"]
mod harness_restart;
#[path = "support/mock_videnoa/mod.rs"]
mod mock_videnoa;

use mock_videnoa::server::MockVidenoa;
use mock_videnoa::{
    checkpoints::Checkpoint,
    domain::{JobProgress as MockJobProgress, JobStatus as MockJobStatus},
    faults::{Fault, OfflineMode, ResponseFault},
    journal::Route,
};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use videnoa_controller::domain::{SubmissionKey, WorkerApiUrl, WorkflowKind, WorkflowName};
use videnoa_controller::remote::{
    sibling_output_path, CacheInvalidation, CapabilityCache, Compatibility, CompatibilityEntry,
    CompatibilityEvidence, FileApiPath, MonotonicClock, PayloadLimits, RemoteTimeouts, RunOutcome,
    VidenoaClient, VidenoaClientError,
};

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

const JSON_LIMIT: usize = 1024 * 1024;
const CHUNK_BYTES: usize = 4 * 1024;

fn test_client(
    server: &MockVidenoa,
    request: Duration,
    stall: Duration,
    json_bytes: usize,
) -> TestResult<VidenoaClient> {
    let base_url = WorkerApiUrl::parse(server.base_url())?;
    let timeouts = RemoteTimeouts::new(Duration::from_secs(2), request, stall)?;
    let limits = PayloadLimits::new(json_bytes, CHUNK_BYTES)?;
    Ok(VidenoaClient::new(base_url, timeouts, limits)?)
}

#[tokio::test]
async fn capabilities_merge_workflows_and_presets_when_interfaces_are_compatible() -> TestResult {
    // Given: a real TCP Videnoa endpoint and bounded client configuration.
    let server = MockVidenoa::start().await?;
    let base_url = WorkerApiUrl::parse(server.base_url())?;
    let timeouts = RemoteTimeouts::new(
        Duration::from_secs(2),
        Duration::from_secs(5),
        Duration::from_secs(1),
    )?;
    let limits = PayloadLimits::new(1024 * 1024, 8 * 1024)?;
    let client = VidenoaClient::new(base_url, timeouts, limits)?;

    // When: health and workflow capabilities are fetched from the real socket.
    let health = client.health().await?;
    let capabilities = client.capabilities().await?;

    // Then: both catalogs are merged and only exact input/output Path interfaces are eligible.
    assert!(health.is_healthy());
    assert_eq!(
        capabilities.compatibility(&WorkflowName::new("eligible-workflow.json")),
        Some(Compatibility::Eligible)
    );
    assert_eq!(
        capabilities.compatibility(&WorkflowName::new("eligible-preset")),
        Some(Compatibility::Eligible)
    );
    assert_eq!(
        capabilities.compatibility(&WorkflowName::new("missing-path.json")),
        Some(Compatibility::Incompatible)
    );
    assert_eq!(
        capabilities.compatibility(&WorkflowName::new("wrong-path-type.json")),
        Some(Compatibility::Incompatible)
    );
    assert_eq!(
        capabilities.compatibility(&WorkflowName::new("no-interface.json")),
        Some(Compatibility::Incompatible)
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn remote_lifecycle_streams_files_and_preserves_workflow_paths() -> TestResult {
    // Given: payloads larger than the configured client chunk and a tracked streaming reader.
    let server = MockVidenoa::start().await?;
    let client = test_client(
        &server,
        Duration::from_secs(5),
        Duration::from_secs(1),
        JSON_LIMIT,
    )?;
    let input: Vec<u8> = (0_u8..=250).cycle().take(CHUNK_BYTES * 5 + 137).collect();
    let read_calls = Arc::new(AtomicUsize::new(0));
    let max_read = Arc::new(AtomicUsize::new(0));
    let reader = TrackedReader::new(
        input.clone(),
        Arc::clone(&read_calls),
        Arc::clone(&max_read),
    );
    let input_path = FileApiPath::parse("task-008/input.mkv")?;

    // When: upload, keyed run, poll, download, stat, cleanup, and cancel use the real TCP API.
    let uploaded = client
        .upload(&input_path, u64::try_from(input.len())?, reader)
        .await?;
    let output_workflow_path = sibling_output_path(&uploaded.path, "output.mp4")?;
    let mut params = BTreeMap::new();
    params.insert(
        "input".to_owned(),
        Value::String(uploaded.path.as_str().to_owned()),
    );
    params.insert(
        "output".to_owned(),
        Value::String(output_workflow_path.as_str().to_owned()),
    );
    let submission_key: SubmissionKey = "00000000-0000-4000-8000-000000000808".parse()?;
    let submission = client
        .run(
            &WorkflowName::new("eligible-workflow.json"),
            submission_key,
            &params,
        )
        .await?;
    server
        .set_job(
            &submission.receipt.id.to_string(),
            MockJobStatus::Running,
            Some(MockJobProgress::new(7, Some(20), 3.5, Some(4.0))),
        )
        .await?;
    let running = client.job(submission.receipt.id).await?;
    let output = (0_u8..=127)
        .cycle()
        .take(CHUNK_BYTES * 6 + 19)
        .collect::<Vec<_>>();
    server
        .complete_job(
            &submission.receipt.id.to_string(),
            "task-008/output.mp4",
            &output,
        )
        .await?;
    let output_path = FileApiPath::parse("task-008/output.mp4")?;
    let mut writer = TrackedWriter::default();
    let downloaded = client.download(&output_path, &mut writer).await?;
    let stat = client.stat(&output_path).await?;
    client.delete_file(&FileApiPath::parse("task-008")?).await?;
    let cancel_key: SubmissionKey = "00000000-0000-4000-8000-000000000809".parse()?;
    let cancel = client
        .run(
            &WorkflowName::new("eligible-preset"),
            cancel_key,
            &BTreeMap::new(),
        )
        .await?;
    client.cancel_job(cancel.receipt.id).await?;

    // Then: paths stay byte-exact, transfers are multi-chunk, and the keyed header reaches the wire.
    assert_eq!(submission.outcome, RunOutcome::Created);
    assert_eq!(
        running.status,
        videnoa_controller::remote::JobStatus::Running
    );
    assert_eq!(
        uploaded.path.as_str(),
        "../mock-worker/workspace/task-008/input.mkv"
    );
    assert_eq!(
        output_workflow_path.as_str(),
        "../mock-worker/workspace/task-008/output.mp4"
    );
    assert!(read_calls.load(Ordering::SeqCst) > 1);
    assert!(max_read.load(Ordering::SeqCst) <= CHUNK_BYTES);
    assert_eq!(writer.bytes, output);
    assert!(writer.write_calls > 1);
    assert!(writer.max_write <= 8 * 1024);
    assert_eq!(downloaded.bytes, u64::try_from(writer.bytes.len())?);
    assert_eq!(stat.size, downloaded.bytes);
    let journal = server.journal().await;
    let run = journal
        .iter()
        .find(|entry| entry.route == Route::Run)
        .ok_or_else(|| std::io::Error::other("run journal entry missing"))?;
    assert!(run
        .headers
        .iter()
        .any(|header| header.name == "idempotency-key"));
    Ok(())
}

#[tokio::test]
async fn json_and_status_failures_are_bounded_typed_and_redacted() -> TestResult {
    // Given: scripted health responses spanning every stable status class and unsafe JSON bodies.
    let server = MockVidenoa::start().await?;
    let client = test_client(&server, Duration::from_secs(5), Duration::from_secs(1), 32)?;
    let cases = [
        (404, VidenoaClientError::NotFound),
        (409, VidenoaClientError::Conflict),
        (429, VidenoaClientError::RateLimited),
        (418, VidenoaClientError::ClientStatus { status: 418 }),
        (503, VidenoaClientError::ServerStatus { status: 503 }),
    ];

    // When/Then: each status maps without reflecting any response body.
    for (status, expected) in cases {
        server
            .set_fault(Fault::Response(ResponseFault {
                route: Route::Health,
                status,
                body: b"sensitive-marker".to_vec(),
            }))
            .await;
        let error = client
            .health()
            .await
            .expect_err("scripted status must fail");
        assert_eq!(error, expected);
        assert!(!format!("{error:?}").contains("sensitive-marker"));
    }
    server
        .set_fault(Fault::Response(ResponseFault {
            route: Route::Health,
            status: 200,
            body: b"{not-json".to_vec(),
        }))
        .await;
    assert_eq!(
        client.health().await.expect_err("malformed JSON must fail"),
        VidenoaClientError::MalformedPayload
    );
    server
        .set_fault(Fault::Response(ResponseFault {
            route: Route::Health,
            status: 200,
            body: vec![b' '; 33],
        }))
        .await;
    assert_eq!(
        client.health().await.expect_err("oversized JSON must fail"),
        VidenoaClientError::OversizedPayload { limit: 32 }
    );
    assert_eq!(
        client
            .workflow_interface(&WorkflowName::new("unknown"))
            .await
            .expect_err("unknown interface must fail"),
        VidenoaClientError::NotFound
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn network_request_stall_and_truncation_errors_remain_distinct() -> TestResult {
    // Given: short configured request/stall bounds and one stored multi-chunk file.
    let mut server = MockVidenoa::start().await?;
    let client = test_client(
        &server,
        Duration::from_millis(75),
        Duration::from_millis(40),
        JSON_LIMIT,
    )?;
    let params = BTreeMap::new();
    let key: SubmissionKey = "00000000-0000-4000-8000-000000000810".parse()?;
    let submitted = client
        .run(&WorkflowName::new("eligible-workflow.json"), key, &params)
        .await?;
    let ticket = server.pause(Checkpoint::BeforePollResponse).await;
    let timeout_client = client.clone();
    let job_id = submitted.receipt.id;
    let timed = tokio::spawn(async move { timeout_client.job(job_id).await });
    server.await_checkpoint(&ticket).await?;

    // When/Then: total request timeout fires while headers are held.
    assert_eq!(
        timed
            .await?
            .expect_err("held response must reach the request timeout"),
        VidenoaClientError::Timeout
    );
    server.release(ticket).await?;

    // When/Then: body stall, truncated body, and refused connection classify independently.
    let path = FileApiPath::parse("fault/large.bin")?;
    server
        .store_file(path.as_str(), &[7_u8; CHUNK_BYTES * 3])
        .await?;
    server.set_fault(Fault::StallDownload).await;
    assert_eq!(
        client
            .download(&path, &mut TrackedWriter::default())
            .await
            .expect_err("stalled body must fail"),
        VidenoaClientError::Stall
    );
    server
        .set_fault(Fault::TruncateDownload { delivered_bytes: 9 })
        .await;
    let truncation_client = test_client(
        &server,
        Duration::from_secs(5),
        Duration::from_secs(1),
        JSON_LIMIT,
    )?;
    assert_eq!(
        truncation_client
            .download(&path, &mut TrackedWriter::default())
            .await
            .expect_err("truncated body must fail"),
        VidenoaClientError::MalformedPayload
    );
    server.set_offline(OfflineMode::ConnectionRefused).await?;
    assert_eq!(
        client.health().await.expect_err("offline socket must fail"),
        VidenoaClientError::Network
    );
    Ok(())
}

#[test]
fn remote_paths_and_tls_configuration_are_parse_only_and_platform_independent() -> TestResult {
    // Given: an HTTPS worker URL and a workflow path containing parent components.
    let worker = WorkerApiUrl::parse("https://worker.example/base")?;
    let timeouts = RemoteTimeouts::new(
        Duration::from_secs(1),
        Duration::from_secs(2),
        Duration::from_secs(1),
    )?;
    let limits = PayloadLimits::new(JSON_LIMIT, CHUNK_BYTES)?;

    // When: the rustls-backed client and sibling output path are constructed without I/O.
    let _client = VidenoaClient::new(worker, timeouts, limits)?;
    let output = sibling_output_path(
        &videnoa_controller::domain::RemotePath::new("../../data/workspace/task/input.mkv"),
        "output.mp4",
    )?;

    // Then: the exact remote parent spelling is preserved and unsafe API paths are rejected.
    assert_eq!(output.as_str(), "../../data/workspace/task/output.mp4");
    assert_eq!(
        FileApiPath::parse("../escape").expect_err("parent path must fail"),
        VidenoaClientError::InvalidFilePath
    );
    Ok(())
}

#[test]
fn capability_cache_expiry_invalidation_and_evidence_priority_are_deterministic() -> TestResult {
    // Given: a deterministic clock, one worker, and a cached eligible workflow.
    let clock = ManualClock::default();
    let worker = WorkerApiUrl::parse("http://worker.example")?;
    let workflow = WorkflowName::new("eligible-workflow.json");
    let catalog = videnoa_controller::remote::CompatibilityCatalog::from_entries([(
        workflow.clone(),
        CompatibilityEntry {
            kind: WorkflowKind::Workflow,
            compatibility: Compatibility::Eligible,
        },
    )]);
    let mut cache = CapabilityCache::new(clock.clone(), Duration::from_millis(100));
    cache.insert(&worker, catalog.clone());

    // When/Then: cache hits before exact expiry and expires at the deterministic TTL boundary.
    assert_eq!(
        cache.resolve(&worker, &workflow, CompatibilityEvidence::default()),
        Some(Compatibility::Eligible)
    );
    clock.advance(Duration::from_millis(99));
    assert_eq!(
        cache.resolve(&worker, &workflow, CompatibilityEvidence::default()),
        Some(Compatibility::Eligible)
    );
    clock.advance(Duration::from_millis(1));
    assert_eq!(
        cache.resolve(&worker, &workflow, CompatibilityEvidence::default()),
        None
    );

    // When/Then: live then durable evidence outrank stale cache, and every invalidation clears it.
    cache.insert(&worker, catalog.clone());
    assert_eq!(
        cache.resolve(
            &worker,
            &workflow,
            CompatibilityEvidence {
                durable: Some(Compatibility::Eligible),
                live: Some(Compatibility::Incompatible),
            },
        ),
        Some(Compatibility::Incompatible)
    );
    assert_eq!(
        cache.resolve(
            &worker,
            &workflow,
            CompatibilityEvidence {
                durable: Some(Compatibility::Incompatible),
                live: None,
            },
        ),
        Some(Compatibility::Incompatible)
    );
    for reason in [
        CacheInvalidation::HealthFailure,
        CacheInvalidation::Restart,
        CacheInvalidation::RemoteError,
    ] {
        cache.insert(&worker, catalog.clone());
        cache.invalidate(&worker, reason);
        assert_eq!(
            cache.resolve(&worker, &workflow, CompatibilityEvidence::default()),
            None
        );
    }
    Ok(())
}

struct TrackedReader {
    bytes: Vec<u8>,
    offset: usize,
    calls: Arc<AtomicUsize>,
    max_read: Arc<AtomicUsize>,
}

impl TrackedReader {
    fn new(bytes: Vec<u8>, calls: Arc<AtomicUsize>, max_read: Arc<AtomicUsize>) -> Self {
        Self {
            bytes,
            offset: 0,
            calls,
            max_read,
        }
    }
}

impl AsyncRead for TrackedReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.max_read
            .fetch_max(buffer.remaining(), Ordering::SeqCst);
        let count = buffer
            .remaining()
            .min(self.bytes.len().saturating_sub(self.offset));
        let end = self.offset + count;
        buffer.put_slice(&self.bytes[self.offset..end]);
        self.offset = end;
        Poll::Ready(Ok(()))
    }
}

#[derive(Default)]
struct TrackedWriter {
    bytes: Vec<u8>,
    write_calls: usize,
    max_write: usize,
}

impl AsyncWrite for TrackedWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        self.write_calls += 1;
        self.max_write = self.max_write.max(bytes.len());
        self.bytes.extend_from_slice(bytes);
        Poll::Ready(Ok(bytes.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[derive(Clone, Default)]
struct ManualClock {
    millis: Arc<AtomicU64>,
}

impl ManualClock {
    fn advance(&self, duration: Duration) {
        self.millis.fetch_add(
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX),
            Ordering::SeqCst,
        );
    }
}

impl MonotonicClock for ManualClock {
    fn now(&self) -> Duration {
        Duration::from_millis(self.millis.load(Ordering::SeqCst))
    }
}
