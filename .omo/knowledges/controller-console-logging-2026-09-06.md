# Controller console diagnostics

## Findings

The standalone Controller previously had no tracing subscriber and only printed two configuration warnings. Task state transitions, reservations, and automatic transfer retries were persisted but invisible in container logs. Upload and download branches also discarded typed remote errors before scheduling retries.

## Behavior

- Initialize timestamped, ANSI-free stderr logging in the binary. Docker captures stderr without a file sink. Default filter: `warn,videnoa_controller=info`; `RUST_LOG` overrides it, with invalid values falling back to the default.
- INFO covers readiness (version, listener, scheduler pause, transfer timeout), orchestration startup, graceful shutdown, committed task creation, reservation, lifecycle transitions, cancellation, and manual retries.
- WARN covers automatic retry count/deadline, unavailable Workers with health retry deadlines, typed remote upload/download failures, and rejected HTTP requests. ERROR covers terminal task failures and HTTP server errors.
- Successful GET/HEAD requests and deferred orchestration diagnostics use DEBUG. Healthy Worker probes and processing progress updates do not generate periodic INFO events.
- State changes log only after a successful database commit, so CAS conflicts and rolled-back idempotency replays cannot claim successful transitions.
- Correlate task and attempt IDs with Worker assignment events. Failure logs contain structured stage/code; arbitrary task failure messages remain in task details because remote messages can contain private data.
- HTTP logs contain the method, matched route template, status, and time to response headers. They do not log raw URLs, queries, request bodies, headers, or streamed response bodies. SSE duration is not measured as a completed stream.
- Task request paths, source references, idempotency keys, and Worker URLs are omitted. Typed remote errors provide timeout/network/HTTP status diagnostics without dumping raw requests.

## Usage

```bash
docker logs -f videnoa-controller
```

For additional request/recovery diagnostics, add `-e RUST_LOG=warn,videnoa_controller=debug` when creating the container. Deploy an image containing the change first; changing logging levels cannot add events to older binaries.

## Regression checks

Tests use synthetic requests and temporary databases, without touching deployment state. Capture tests verify default verbosity, invalid-filter fallback, committed creation versus rolled-back replay, and omission of private request values from HTTP and task logs.

```bash
cargo +1.83.0 test --locked -p videnoa-controller
cargo +1.83.0 clippy --locked -p videnoa-controller --all-targets --all-features -- -D warnings
cargo +1.83.0 fmt --all -- --check
```

Verified locally: 504 Controller tests passed, strict Clippy and formatting passed, and the README contract passed. A freshly built standalone process in a temporary workspace also emitted default readiness, orchestration, and SIGTERM shutdown messages to stderr without ANSI escapes.

## Task HTTP error details

Task errors now attach explicitly constructed diagnostics to response extensions. The HTTP logging middleware records those diagnostics alongside method, route, status, and latency; it never reads or buffers response bodies. Single-task diagnostics match the existing public error envelope, including error code/message, retryability, and field validation details. These messages and field names are generated from Controller validation rules, not raw request values.

Batch creation attaches created/failed counts and up to 16 error entries with zero-based response item indices. Additional errors are counted in `omitted_errors`; the actual HTTP response retains all items. Preview-gate rejection (400) and partial admission (207) both log at WARN, while successful creation stays at INFO. Task authentication errors, malformed requests, and idempotency conflicts use the same error envelope path.

Targeted verification uses synthetic requests and temporary databases:

```bash
cargo +1.83.0 test --locked -p videnoa-controller --lib --test task_api --test task_api_concurrency
cargo +1.83.0 clippy --locked -p videnoa-controller --all-targets --all-features -- -D warnings
```

Regression coverage checks public-error/diagnostic parity for every task error variant, default-level visibility of validation details and 207 failures, bounded batch diagnostic size, and omission of private request paths.
