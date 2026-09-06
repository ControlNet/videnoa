# Videnoa Controller Task 2 Knowledge

## Contract Modules

- `crates/controller/src/domain/` owns only stable values and HTTP/persistence DTOs: branded identifiers, exact path/workflow/source values, lifecycle/failure/query enums, task/attempt/progress models, worker/settings/auth/action/system schemas, SSE events, and typed API errors.
- Task 2 defines no repositories, schedulers, remote clients, authentication logic, or operational route handlers.
- `TaskCreateRequest` has no update counterpart for paths. It preserves exact input/output strings, workflow, priority, source, and optional source reference; later tasks validate filesystem capabilities without rewriting the request.

## Stable Vocabulary

- Lifecycle JSON values are exactly `queued`, `reserved`, `uploading`, `staged`, `submitting`, `processing`, `remote_completed`, `downloading`, `verifying`, `publishing`, `remote_cleanup`, `completed`, `failed`, and `cancelled`.
- Task sources are `manual` and `api`.
- Sort fields are `priority`, `created_at`, `completed_at`, `status`, `worker`, and `duration`; directions are `asc` and `desc`.
- Filters cover status, worker, workflow, source, failure stage, and search.
- Page defaults are limit `100`, maximum `500`, and offset `0`.

## Configuration Boundary

- Layering order is serialized defaults, optional exact TOML, then `VIDENOA_CONTROLLER_` environment overrides using `__` for nested sections, for example `VIDENOA_CONTROLLER_SCHEDULER__MAX_CONCURRENT_UPLOADS=3`.
- Every raw config section uses `serde(deny_unknown_fields)`. Unknown TOML and prefixed environment keys fail as `ConfigError::Schema`.
- Validation rejects missing/non-directory/symlink roots, missing/non-file/symlink hash files, zero ports/slots/concurrency/timeouts/retry attempts, numeric overflow, idle sessions longer than absolute sessions, and initial retry delay above maximum.
- Defaults: host `127.0.0.1`, port `3001`, input `input`, output `output`, data `data`, temp `data/temp`, hash file `data/admin-password.phc`, secure cookie enabled, sessions `24h/1h`, slots/prefetch/uploads/downloads `1/1/1/1`, health/poll/transfer `10s/5s/300s`, retry `1s/60s/5`.

## Secret and URL Safety

- Runtime configuration stores only a password-hash file path.
- `LoginRequest` is deserialize-only; `SecretString` renders as `[redacted]` in `Debug` and is excluded from Task 2 evidence.
- Worker API URLs accept only HTTP(S), reject credentials/query/fragment, and serialize through `url::Url` with one trailing slash.

## Evidence

- Red compile evidence: `.omo/evidence/videnoa-controller/task-2/red.txt`.
- Public DTO fixture: `.omo/evidence/videnoa-controller/task-2/contracts.json`.
- Stable invalid-boundary summary: `.omo/evidence/videnoa-controller/task-2/config-errors.txt`.

## Verification

- Current release formatting, strict Clippy, tests, and build pass; Rust 1.83 compiles and passes all Controller tests.
- Existing `videnoa-core` and full-workspace regressions pass, including 590 core tests with 10 intentional ignores.
- Frontend install, lint, Vitest, TypeScript, and Vite production build pass when run before the release Cargo gate.
- The direct Controller dependency tree contains no `videnoa-core`, ORT, CUDA, cuDNN, or TensorRT path.
- Live release probes return typed health and API 404 JSON, reject ambiguous paths with 400, and serve the embedded SPA at `/`.
