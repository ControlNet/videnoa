# Deep project review, second pass (2026-11-13)

Reviewed `dev` at `66bae59`. No application source changed. These findings are
additional to `deep-review-2026-11-13.md`. Tests use temporary files, a loopback
HTTP listener, and an explicitly synthetic marker, never real credentials.

## Findings

### P1: HTTP errors retain sensitive fields in JSON request bodies

`crates/core/src/nodes/http_request.rs:196-203` interpolates the complete request
body into error context. `crates/core/src/logging.rs:477-500` looks backwards
from a colon or equals sign for an unquoted key. In JSON the character before
the colon is a quote, so the key is not recognized and its value is not masked.
The same text redactor is used for node errors and logging.

Reproduced using a synthetic JSON password field and a timed-out POST: the
returned error still contains the marker. No marker contents or error body were
printed. `server/mod.rs:3460-3464` stores formatted executor errors on the job
and logs them, so the code path can propagate a real supplied secret to job
history and logs. The node error was tested; database/log propagation was traced
in source, not separately exercised with a real secret.

Fix: omit raw bodies from error messages; use structured recursive redaction
where diagnostics require JSON fields, and avoid exposing unparseable raw
payloads. Add tests for quoted JSON keys, nested objects and arrays.

### P1: small workflow documents can cause unbounded allocation during validation

`nodes/string_template.rs:17-24` accepts any nonnegative i64 `num_input`.
`input_ports()` at lines 55-86 loops over that value, allocating one named port
per iteration. `graph.rs:325` calls it during validation, before admission's queue
capacity checks and outside the executor's panic handling. Request-body size
limits therefore do not bound validation's memory or CPU cost.

A safe probe with only 10,000 requested inputs produced 10,003 ports and passed
graph validation. No OOM or resource exhaustion was intentionally triggered.
The whole-process OOM/availability consequence at much larger values is inferred
from the unbounded allocation path. A single bad numeric value is enough; this
does not require sending a large graph document.

Fix: reject unsupported port counts before allocation; bound both per-node
expansion and total validation work. Apply the same review to model worker counts
and tensor dimensions, rather than relying on a job-count limit.

### P2: HttpRequest retries POST after ambiguous delivery, including after cancel

`nodes/http_request.rs:205-220` retries every retryable transport error regardless
of HTTP method. Default max_retries is 2. A timeout does not establish that the
remote side did not perform the request. The execution context is named `_ctx`
and is never consulted inside the retry loop.

A loopback server accepted POST requests but withheld responses. One execute call
produced three server-observed POSTs. Repeating with a cancellation watch set
immediately after the first accepted POST still produced three POSTs. These are
new requests, not merely slow completion of an already-running request.

Fix: default non-idempotent operations to no automatic ambiguous-delivery retry;
allow retries only under an explicit application idempotency contract. Check
cancellation before every attempt and during backoff. The broader limitation of
network-node cooperative cancellation was noted in the September 26 review;
this pass supplies a concrete post-cancellation retry reproduction.

### P2 design gap: a saved job does not snapshot its nested workflow dependencies

`nodes/workflow_io.rs:292` reloads the inner workflow from its path at execution.
The persisted outer PipelineGraph contains that path and interface, not the
nested graph bytes or a content identity. This is distinct from incomplete inner
graph validation reported in the first pass.

Reproduced: validate an outer graph, serialize it as a persisted snapshot, change
the inner file's constant, deserialize the unchanged outer snapshot, execute it.
The result is the changed value. Queueing gives users time to edit the referenced
file between admission and execution; rerun also resolves the latest file. Job
history therefore cannot identify all code that actually ran.

Fix: define this policy explicitly. For reproducible jobs, recursively snapshot
resolved dependencies, or pin hashes and reject changed dependencies. If live
references are intentional, expose this distinction and record the resolved
execution content/identity. The test proves by-reference behavior, not a breach
of an explicitly documented immutability guarantee.

### P3: Resize preview silently truncates dimensions that full execution rejects

`nodes/resize.rs:98-112` checks positivity then uses `as u32`. A requested width
of 4,294,967,297 becomes 1. The node-level probe processed a one-pixel frame and
returned width 1 without error. `server/preview.rs:125-129` uses this ResizeNode.
Full video compilation uses `read_required_positive_u32` in
`nodes/compile_context.rs:1221-1227`, which correctly rejects the same value with
`u32::try_from`. Thus preview and execution disagree for out-of-range inputs.

Fix: share checked dimension parsing between preview and full execution, plus
appropriate pixel/byte allocation limits. This is an extreme-input bug; ordinary
video resolutions are unaffected. No giant image allocation was performed.

## Excluded candidates and limits

- Controller uploading current source content is explicitly authorized in
  `controller-current-input-upload-2026-09-07.md`. Do not restore admission-time
  identity checks as a purported bug fix. README-controller.md still describes
  changed input identity as fail-closed without clearly delimiting intake; its
  wording could be clarified, but this is not counted as a new code bug.
- Changes to worker endpoints with outstanding tasks were already covered by the
  October 23 review; not re-reported here.
- No live remote service, GPU inference, browser E2E or OOM test was run.

## Verification

```sh
cargo test -p videnoa-core --test review_second_probe -- --nocapture
cargo test -p videnoa-core --lib nodes::http_request::tests -- --test-threads=1
```

- Review probes: 4 passed, 0 failed (1.10 seconds). One probe covers retries,
  cancellation and synthetic-body redaction. Assertions confirm current defects.
- Existing HTTP-node unit tests: 9 passed, 0 failed (0.38 seconds).
- First-pass full-suite results remain in its note; they were not rerun or
  relabeled in this pass.

The temporary integration test was removed after saving its complete source
below. To repeat, save the block to `crates/core/tests/review_second_probe.rs`
and run the commands above, then remove that temporary file.

## Synthetic probe source

```rust
use std::{collections::HashMap, io::Read, net::TcpListener, sync::{Arc, atomic::{AtomicUsize, Ordering}}, time::{Duration, Instant}};
use serde_json::json;
use videnoa_core::{node::{Node, ExecutionContext, FrameProcessor}, types::{PortData, Frame}, graph::PipelineGraph, registry::build_default_registry, executor::SequentialExecutor};

#[test]
fn post_is_repeated_even_after_cancellation() {
    for cancel in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/operation", listener.local_addr().unwrap());
        let count = Arc::new(AtomicUsize::new(0));
        let seen = count.clone();
        let (tx, rx) = tokio::sync::watch::channel(false);
        let server = std::thread::spawn(move || {
            let end = Instant::now() + Duration::from_secs(3);
            let mut sockets = Vec::new();
            while Instant::now() < end && seen.load(Ordering::SeqCst) < 3 {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
                        let mut bytes = [0; 2048];
                        let n = stream.read(&mut bytes).unwrap();
                        assert!(String::from_utf8_lossy(&bytes[..n]).starts_with("POST "));
                        seen.fetch_add(1, Ordering::SeqCst);
                        if cancel { tx.send_replace(true); }
                        sockets.push(stream);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(2)),
                    Err(e) => panic!("{e}"),
                }
            }
            std::thread::sleep(Duration::from_millis(250));
        });
        let ctx = ExecutionContext { cancellation: Some(rx), ..Default::default() };
        let mut node = videnoa_core::nodes::http_request::HttpRequestNode::new();
        let inputs = HashMap::from([
            ("url".into(), PortData::Str(url)), ("method".into(), PortData::Str("POST".into())),
            ("body".into(), PortData::Str(json!({"password":"REVIEW_MARKER_NOT_A_CREDENTIAL"}).to_string())),
            ("timeout_ms".into(), PortData::Int(100)), ("retry_backoff_ms".into(), PortData::Int(0)),
        ]);
        let error = node.execute(&inputs, &ctx).err().unwrap().to_string();
        assert!(error.contains("REVIEW_MARKER_NOT_A_CREDENTIAL"));
        println!("synthetic JSON body marker survives error redaction: true");
        server.join().unwrap();
        println!("cancel={cancel}, server-observed POST count={}", count.load(Ordering::SeqCst));
        assert_eq!(count.load(Ordering::SeqCst), 3);
    }
}

#[test]
fn nested_snapshot_reads_changed_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("inner.json");
    let inner = |value: &str| json!({"nodes":[
        {"id":"c","node_type":"Constant","params":{"type":"Str","value":value}},
        {"id":"o","node_type":"WorkflowOutput","params":{"ports":[{"name":"value","port_type":"Str"}]}}
    ],"connections":[{"from_node":"c","from_port":"value","to_node":"o","to_port":"value","port_type":"Str"}]});
    std::fs::write(&path, inner("admitted").to_string()).unwrap();
    let graph: PipelineGraph = serde_json::from_value(json!({"nodes":[{"id":"nested","node_type":"Workflow","params":{"workflow_path":path,"interface_outputs":[{"name":"value","port_type":"Str"}]}}],"connections":[]})).unwrap();
    let registry = build_default_registry();
    graph.validate(&registry).unwrap();
    let saved = serde_json::to_vec(&graph).unwrap();
    std::fs::write(&path, inner("changed").to_string()).unwrap();
    let restored: PipelineGraph = serde_json::from_slice(&saved).unwrap();
    let result = SequentialExecutor::execute(&restored, &registry).unwrap();
    match &result["nested"]["value"] {
        PortData::Str(value) => { println!("saved graph executed inner value={value}"); assert_eq!(value,"changed"); }
        _ => panic!("expected string"),
    }
}

#[test]
fn validation_expands_unbounded_port_count() {
    let node = videnoa_core::nodes::string_template::StringTemplateNode::from_params(&HashMap::from([("num_input".into(),json!(10000))]));
    assert_eq!(node.input_ports().len(),10003);
    let graph: PipelineGraph = serde_json::from_value(json!({"nodes":[{"id":"t","node_type":"StringTemplate","params":{"num_input":10000}}],"connections":[]})).unwrap();
    graph.validate(&build_default_registry()).unwrap();
    println!("small graph passed validation after expanding 10003 ports");
}

#[test]
fn resize_dimension_wraps_to_one_pixel() {
    let mut node = videnoa_core::nodes::resize::ResizeNode::new();
    let ctx = ExecutionContext::default();
    node.execute(&HashMap::from([("width".into(),PortData::Int(4294967297)),("height".into(),PortData::Int(1)),("algorithm".into(),PortData::Str("nearest".into()))]), &ctx).unwrap();
    let frame = Frame::CpuRgb { data: vec![1,2,3], width:1, height:1, bit_depth:8 };
    match node.process_frame(frame,&ctx).unwrap() {
        Frame::CpuRgb { width, height, .. } => { println!("requested width=4294967297, actual={width}x{height}"); assert_eq!((width,height),(1,1)); }
        _ => panic!("expected CPU frame"),
    }
}

```
