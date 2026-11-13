# Deep project review (2026-11-13)

Reviewed `dev` at `fe2455a`. Application source was not changed. Compared the
October 23/29/30 and November 4 reviews with current fixes before reporting.
Synthetic probes used temporary graphs and a temporary SQLite database, not real
media or production state. The full probe source is preserved below.

## Findings

1. **P1: worker terminal state is published before durable persistence succeeds.**
   `crates/core/src/server/mod.rs:3441-3483` mutates the in-memory job first and
   only logs `persist_job_transition` errors. There is no retry or dirty-state
   tracking. A temporary SQLite trigger rejecting `completed` updates reproduced
   API `completed` with durable `running`. Startup at
   `server/persistence.rs:203-220` converts that row to `cancelled`. A controller
   that reconnects after restart can therefore lose a successfully computed
   result. Persist a terminal transition before advertising it, retain/retry
   failed writes, and expose degraded persistence rather than silently claiming
   durable completion. The restart consequence is established by code inspection;
   the API/database disagreement was reproduced.

2. **P2: scalar input ports accept multiple incoming edges and silently choose one.**
   `crates/core/src/graph.rs:75-94,132-218` checks individual types but not target
   uniqueness; `executor.rs:164-182` inserts into a map keyed by target port.
   `web/src/pages/editor/EditorPage.tsx:216-280` checks types only and appends
   edges, so ordinary editor actions can create this graph. Two different string
   constants feeding one WorkflowOutput passed validation; reversing just the
   connections array changed the result from `first` to `second`. Other scalar
   ports include output paths, model paths and encoder settings. Reject duplicate
   target ports on the server and replace/refuse an occupied connection in the UI.

3. **P2: nested workflow prevalidation does not validate the inner graph.**
   `crates/core/src/nodes/workflow_io.rs:384-403` checks frame edges and nested
   references, but never checks the loaded graph's node registry, ports, required
   inputs or DAG validity. A nested `MissingNodeType` passed outer `validate()`
   and failed only in execution. Submission invokes that outer validation in
   `server/mod.rs:1757-1762`. Recursively validate with a shared recursion context
   (avoid re-entering the same validator with a reset recursion chain), and verify
   the declared outer interface against inner WorkflowInput/WorkflowOutput ports.

4. **P2: queue capacity check and admission are not atomic.**
   `crates/core/src/server/mod.rs:413-424` counts existing active jobs; submission
   later persists and inserts jobs at `1773-1802` and `2012-2036`. On the
   multithread runtime, simultaneous submissions can both pass the 64-job limit.
   Batches enlarge the overshoot because none of their jobs enter the runtime map
   until all individual persistence calls finish. The persistence mutex does not
   cover the earlier capacity check. This is a code-path finding, not a load-test
   result. Reserve capacity with owned permits or one admission critical section,
   including batch rollback and shutdown admission.

5. **P2: overwriting a workflow truncates the only saved copy.**
   `crates/core/src/server/mod.rs:2450-2452` calls `std::fs::write` on the existing
   file. Disk-full/write failures or interruption after truncation lose the old
   graph and can leave malformed JSON; a concurrent run can read an incomplete
   document. This remains after the filename/overwrite-confirmation fixes.
   Write a temporary file in the same directory, flush/sync as appropriate and
   atomically replace the original. Concurrent editors also need a version/hash
   precondition if lost updates are to be prevented. Code inspection only; no
   crash or disk-full injection was performed.

6. **P2: interrupted staged-output cleanup misses computed and relative paths.**
   `crates/core/src/server/staged_outputs.rs:48-97` reconstructs output locations
   only from literal VideoOutput params or direct WorkflowInput connections, and
   discards relative paths. A supported PathDivider/PathJoiner-derived output
   (see `crates/core/tests/workflow_validation.rs`) leaves its staged encode after
   a hard stop. Startup first makes the job terminal, so later restarts do not
   retry that cleanup. WorkflowInput port defaults are also not resolved here.
   This is a confirmed coverage gap by inspection, not a crash reproduction.
   Persist the actual staging paths at creation, with the existing job ownership
   checks, and retain a cleanup obligation until removal succeeds.

## Existing limitations rechecked

- Streaming decoder reads and encoder writes can block outside cooperative
  cancellation (`nodes/video_output.rs:764`); do not describe cancellation as a
  hard deadline. Already documented in the October 23 review.
- Dynamic-shape TensorRT engine creation happens on first inference after the
  cache-entry lock is released; dynamic-shape broken-engine recovery remains
  absent. Already documented in `engine-review-fixes-2026-10-30.md`.
- No GPU inference, cross-platform build, real power-loss, or real browser E2E
  run was performed in this review. Passing existing tests does not cover the
  fault-injection and graph edge cases above.

## Verification

Commands run from the repository root:

```sh
cargo test -p videnoa-core --lib --quiet
cargo test -p videnoa-controller --quiet
npm --prefix web test -- --reporter=dot
npm --prefix controller-web test -- --reporter=dot
cargo test -p videnoa-core --test review_20261113_probe -- --nocapture
```

- Core library: 881 passed, 24 ignored.
- Worker frontend: 37 files, 299 tests passed.
- Controller frontend: 30 files, 196 tests passed.
- Targeted review probes: 3 passed. These assertions confirm current bugs,
  rather than asserting the desired corrected behavior.
- Controller full command exited 101 in `videnoa_client`: 37 passed and
  `upload_response_headers_stall_after_body_eof` failed with
  `CheckpointTimeout("after_upload_bytes_accepted")`. Earlier groups completed,
  including all 71 task20 fault-recovery tests (387.10 seconds).
- The failed test immediately passed in isolation (0.10 seconds). Its 100 ms
  stall deadline starts before it reaches the checkpoint it intends to test;
  scheduling/connection delay can expire that deadline first. This is consistent
  with timing sensitivity, but the precise cause of this run was not traced.
  Do not report the original full command as passing. The entire client test
  binary then passed serially: 38 passed, 0 failed, 7.79 seconds.

Targeted repeat command:

```sh
cargo test -p videnoa-controller --test videnoa_client upload_response_headers_stall_after_body_eof -- --exact --nocapture
cargo test -p videnoa-controller --test videnoa_client -- --test-threads=1
```

To repeat the probes, save the Rust block below as
`crates/core/tests/review_20261113_probe.rs`, run the final command above, and
remove that temporary file afterwards. The review itself removes the temporary
integration-test file and commits only this knowledge note.

## Synthetic probe source

```rust
use serde_json::json;
use videnoa_core::{graph::PipelineGraph, registry::build_default_registry, executor::SequentialExecutor};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn terminal_persistence_failure_still_reports_completed() {
    use axum::{body::{Body, to_bytes}, http::Request};
    use tower::ServiceExt;
    use videnoa_core::{config::AppConfig, server::{api_router, app_state_with_config}};
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let state = app_state_with_config(AppConfig::default(), dir.path().join("config.toml"), data.clone());
    let db = rusqlite::Connection::open(data.join("jobs.db")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_terminal BEFORE UPDATE ON jobs WHEN NEW.status = 'completed' BEGIN SELECT RAISE(ABORT, 'review injected terminal write failure'); END;").unwrap();
    let app = api_router(state);
    let response = app.clone().oneshot(Request::builder().method("POST").uri("/api/jobs").header("content-type","application/json").body(Body::from(json!({"workflow":{"nodes":[{"id":"a","node_type":"Constant","params":{}}],"connections":[]}}).to_string())).unwrap()).await.unwrap();
    assert_eq!(response.status(), 201);
    let created: serde_json::Value = serde_json::from_slice(&to_bytes(response.into_body(), 1048576).await.unwrap()).unwrap();
    let id = created["id"].as_str().unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let response = app.clone().oneshot(Request::builder().uri(format!("/api/jobs/{id}")).body(Body::empty()).unwrap()).await.unwrap();
            let value: serde_json::Value = serde_json::from_slice(&to_bytes(response.into_body(),1048576).await.unwrap()).unwrap();
            if value["status"] == "completed" { break; }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let status: String = db.query_row("SELECT status FROM jobs WHERE id = ?1", [id], |row|row.get(0)).unwrap();
    println!("API status completed, durable status {status}");
    assert_eq!(status,"running");
}

#[test]
fn nested_invalid_node_passes_outer_validation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("inner.json");
    std::fs::write(&path, json!({"nodes":[{"id":"bad","node_type":"MissingNodeType","params":{}}],"connections":[]}).to_string()).unwrap();
    let outer: PipelineGraph = serde_json::from_value(json!({"nodes":[{"id":"nested","node_type":"Workflow","params":{"workflow_path":path}}],"connections":[]})).unwrap();
    let registry = build_default_registry();
    assert!(outer.validate(&registry).is_ok());
    let error = SequentialExecutor::execute(&outer, &registry).err().unwrap();
    println!("Outer validation passed; execution failed: {error:#}");
}

#[test]
fn duplicate_input_edges_are_accepted_and_order_changes_result() {
    let registry = build_default_registry();
    let mut values = Vec::new();
    for sources in [["a", "b"], ["b", "a"]] {
        let graph: PipelineGraph = serde_json::from_value(json!({
            "nodes":[
                {"id":"a","node_type":"Constant","params":{"type":"Str","value":"first"}},
                {"id":"b","node_type":"Constant","params":{"type":"Str","value":"second"}},
                {"id":"out","node_type":"WorkflowOutput","params":{"ports":[{"name":"value","port_type":"Str"}]}}
            ],
            "connections":sources.map(|source|json!({"from_node":source,"from_port":"value","to_node":"out","to_port":"value","port_type":"Str"}))
        })).unwrap();
        graph.validate(&registry).unwrap();
        let result = SequentialExecutor::execute(&graph, &registry).unwrap();
        match &result["out"]["value"] {
            videnoa_core::types::PortData::Str(value) => values.push(value.clone()),
            _ => panic!("expected string"),
        }
    }
    println!("Same edges, reversed serialization order: {values:?}");
    assert_ne!(values[0],values[1]);
}

```
