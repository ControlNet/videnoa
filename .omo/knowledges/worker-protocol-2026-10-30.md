# Worker protocol 2 and the videnoa-protocol crate (2026-10-30)

Design-review fixes F (no version channel or shared types), S (deterministic
worker 500 before acceptance retried forever), the Low "cancelling a Submitting
task probes with /api/run", and wave-1 carry-overs c1 to c4.

## Shared wire types

- `crates/protocol` (`videnoa-protocol`) holds every type the worker writes and
  the Controller reads: health, jobs, run request/receipt, workflows, presets,
  interfaces, files, errors. Deps: serde, serde_json, chrono only.
- Core re-exports them from `server/mod.rs`; core keeps its own `JobStatus` and
  `CancelReason` with `From` impls into the protocol enums.
- The Controller wraps them in `remote/dto.rs` (`serde(from/try_from)`), so
  domain types (uuid ids, `WorkflowInterface`) stay on the Controller side.
- The mock worker (`tests/support/mock_videnoa/domain.rs`) re-exports the same
  types: a drift fails to compile.
- `crates/protocol/tests/fixtures/v0.1.10/*.json` round-trip byte for byte.
  serde_json `Value` sorts object keys, so fixture keys inside a `Value`
  (preset `workflow`, job `params`) must be written sorted.

## Version channel

- `/api/health` adds `protocol` (missing = 1). `PROTOCOL_VERSION = 2`.
- The health probe (`workers/health/probe.rs`) and the reconciler health gate
  (`recovery/reconciler.rs`) put a worker with a newer protocol offline/deferred.
- `status` must stay the first health field: the Docker contract script greps
  `^{"status":"ok"[,}]`.
- `AssignedWorker.protocol` carries the version into recovery. Gate a new
  endpoint on `protocol >= 2`, not on a 404: the worker's
  `api_route_not_found` fallback makes a protocol-1 404 indistinguishable.

## Typed error codes (S, c4)

- Worker: `AppError::with_code(code)` turns any plain variant into
  `AppError::Rejected` with the same status and message.
- Controller: `VidenoaClientError::ClientStatus` gained `code: Option<ErrorCode>`
  (a field, not a new variant, so other owners' exhaustive matches still hold).
  `remote/jobs.rs::run_rejection` maps code first: permanent codes become a
  `ClientStatus` even for a 500 (terminal `RemoteSubmissionFailed`),
  `queue_full`/`shutting_down` become `ServerStatus` (transient), unknown codes
  fall back to the status. A 422 means the worker cannot parse the request.
- `transport.rs::forbidden` reads 403 bodies; only `host_not_allowed` keeps its
  reason. `is_authentication()` excludes it; `is_worker_level()` includes it, so
  it never fails a task. The probe shows the worker's reason with normal backoff.

## Cancellation (lookup, c2, c3)

- `GET /api/jobs/by-idempotency-key/{key}` reads the persisted mapping
  (`JobsPersistence::idempotent_job`) and returns the `CreateJobResponse` the
  run replay would. It never creates a job. Remaining race, same as before: a
  run still in flight from a crashed Controller can land after the lookup said
  "not accepted".
- `cancel_submission` uses the lookup on protocol 2 and the keyed run replay on
  protocol 1. A non-worker-level `ClientStatus` from the lookup is mapped to
  `UnexpectedStatus` so only the coded 404 proves "not accepted".
- `stop_remote_job` (recovery/submission.rs): DELETE, then GET. Active job ->
  defer the task (its workspace stays); finished -> DELETE again to drop the
  history; 404 -> done. Only then `finish_cancellation` deletes the workspace.
- `processing.rs::cancelled_failure` maps `cancel_reason` to the message.
- Mock: `Fault::SlowJobCancellation` keeps an active job listed with
  `cancel_requested` after DELETE; `MockVidenoa::request_cancel` settles it.

## Per-task cooldown (c1)

`recovery/worker.rs::defer_remote`: worker-level errors back the worker off;
anything else (400/409 on the job's own URL, NotFound, malformed) only defers
the task (`report.defer`, 1 s cooldown via `not_before`).

## controller-web

Response schemas are no longer `.strict()` (unknown keys are stripped); request
schemas stay strict. Lists (`taskListSchema`, `taskStatusCountsSchema`,
`workerListSchema`) parse row by row through `rowsOf` in `src/api/schemas.ts`,
which leaves out (and `console.warn`s) rows it cannot read. Settings groups are
loose for responses and `.strict()`ed inside `settingsUpdateRequestSchema`.

## Verification

```bash
cargo test --locked -p videnoa-protocol
cargo test --locked -p videnoa-controller --test task20 -- worker_protocol remote_scope
# core needs the runtime env from AGENTS.md
cargo test --locked -p videnoa-core --lib -- server::
cd controller-web && npm run lint && npm test -- --run && npm run build
```
