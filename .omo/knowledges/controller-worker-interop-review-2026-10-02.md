# Controller-Worker interop review of the v0.1.7..dev Worker changes (2026-10-02)

What the Controller relies on, and which Worker changes touched it.

## Contract surface

- Endpoints: `POST /api/run` (idempotency key; 201 Created / 200 Replayed),
  `GET` + `DELETE /api/jobs/{id}`, `GET /api/health`, `GET /api/workflows`,
  `GET /api/presets`, `/api/files` (upload, stat, download).
- Every response DTO in `crates/controller/src/remote/dto.rs` uses
  `deny_unknown_fields`: adding a field to a Worker response breaks the
  Controller. Check `Job`, `RunReceipt`, `Health`, `Workflow`, `Preset`,
  `UploadReceipt`, `FileStat` against any Worker serializer change.
- JSON bodies are capped at 1 MiB (`RECOVERY_JSON_LIMIT`); a larger poll
  response is `OversizedPayload` -> `remote_state_ambiguous`. The Worker's
  ffmpeg stderr tail in `job.error` is bounded to 4 KiB (`STDERR_TAIL_BYTES`).
- The Controller never deletes Worker job records except on cancellation
  (see `job-history-limit-and-sse-fanout-2026-10-01.md`).

## Findings

- History limit: reverted (`1a7d75b`).
- Request-head deadline closes idle keep-alive connections at 30 s; reqwest
  pooled them for 90 s -> rare `IncompleteMessage` on reuse; a failed health
  probe marks the Worker offline. Fixed with a 20 s pool idle timeout
  (`ad6ac39`, details in `http-header-read-timeout.md`).
- Submission-time validation (`3879ec1`): `POST /api/run` now rejects invalid
  frame chains with 400 before queueing. Validation runs on the saved workflow
  before param injection; inputs fed by `WorkflowInput` connections count as
  satisfied, and every bundled preset passes
  (`bundled_presets_pass_run_validation_before_param_injection`).
  A 400 at submission is terminal `remote_submission_failed` (retry blocked),
  where the same workflow used to fail in processing (retryable). The Worker's
  JSON `error` text is now carried in `VidenoaClientError::ClientStatus.reason`
  for a 400 from `/api/run` only: control characters become spaces, cut to
  1024 chars plus `…`. All other statuses, routes and non-JSON bodies stay
  redacted (`json_and_status_failures_are_bounded_typed_and_redacted`).
- No change: response structs, `files.rs`, idempotency, auth; encoder check at
  submission uses the cached `ffmpeg -encoders` list (NVENC probe runs only at
  job compile); HTTP/1-only is fine (reqwest speaks HTTP/1.1 over plain HTTP);
  the iroh loopback listener still uses `axum::serve`.

## Test notes

- `crates/controller/tests/task20.rs` (real HTTP, ~4 min) has checkpoint
  timing flakes under local load; a full `cargo test --workspace` can also hit
  a ~30 s stall that shows as `PoolTimedOut` across `task11`. Rerun the binary
  alone before suspecting a change.
