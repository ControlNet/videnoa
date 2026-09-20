# Recovery worker version conflict can terminate the controller

## Evidence and cause

A production report showed `Controller shutdown started`, `Controller stopped with an error`, and `Error: worker changed since it was read`. The later startup advertised version 0.1.3; that version string does not identify the exact crashed build. These logs do not identify the competing writer or establish a storage timeout.

The current recovery path reads a worker, awaits its remote health response, and uses that snapshot version to persist health backoff after a transient failure. A registration edit, background health update, or another task recovery can change the version during the network await. WorkerRegistry::refresh_health then returns Conflict. Reconciler::defer_worker previously propagated it through RecoveryError::Worker. StageError::retryable classifies that wrapper as fatal, allowing the orchestration error to reach the main runtime and initiate coordinated shutdown with a nonzero process exit. Background WorkerHealthService already discards stale health results; this recovery path lacked equivalent conflict handling.

## Correction

Handle only WorkerRegistryError::Conflict in defer_worker as superseded health evidence and defer the task. Preserve the newer worker record and current task assignment. A later reconciliation reloads the worker. Actual persistence failures and other registry errors remain errors; they are not silently swallowed.

The deterministic regression uses an isolated synthetic HTTP worker and real SQLite storage. A new BeforeHealthResponse test checkpoint holds a 503 response after recovery reads the worker. The test disables the worker using the registry, releases the response, and verifies successful deferral, unchanged newer registration/health data, unchanged task version/status, and one retained compute slot. A subsequent healthy reconciliation polls the existing task. Assigned tasks continue reconciliation even when new scheduling on that worker is disabled.

Before the fix, the regression failed with Worker(Conflict). After the fix, it passed. No production controller or worker was restarted or modified by these diagnostics.

## Verification commands

```bash
TMPDIR=/run/user/1008 cargo test -p videnoa-controller --test mock_videnoa concurrent_worker_edit -- --nocapture
TMPDIR=/run/user/1008 cargo test -p videnoa-controller --no-fail-fast -- --test-threads=4
cargo clippy -p videnoa-controller --all-targets -- -D warnings
```

This host uses /run/user/1008 for test temporary storage to avoid variable VM disk synchronization latency while preserving a different filesystem from /dev/shm for cross-mount tests. The injected 503 worker is test-only; no production task input or credentials are involved.

Final verification: the full controller command exited 0 with 583 passed, 0 failed, and 1 pre-existing ignored stress test. Strict all-target Clippy, formatting checks for all modified Rust files, and staged whitespace/secret scans passed.
