# F1 Plan Compliance and Scope-Fidelity Audit

Audit date: 2026-09-04

Audited tip: `ca0b27e7f3bd07a394903504a39b35a759a8f321`

Controller implementation baseline: `5c099b1f71d10920bc054f0994452d0b937294b1`

Repository state at audit start: `HEAD == origin/dev`, divergence `0 0`, clean detached audit worktree.

## Verdict

**REJECT**

The implementation covers most of the approved architecture and passes the Controller Rust, frontend, browser, container, archive, documentation, and workflow contract suites. However, the production service has no worker health and capability discovery loop. A worker created through the authenticated API is durably inserted as `online = false` with empty capabilities, and no production caller invokes `VidenoaClient::capabilities()` or persists a successful `WorkerHealthUpdate`. Atomic reservation requires `worker.online = 1` and a compatible persisted workflow. Therefore an operator following the delivered Workers UI/API cannot make a newly registered Videnoa instance schedulable. Tests bypass this missing runtime responsibility by writing `WorkerHealthUpdate` directly.

This violates Tasks 8, 10, and 11 and the success criterion that an operator can register one or more Videnoa instances and schedule only online, workflow-compatible workers. It is a user-visible missing orchestration stage, so F1 cannot approve the candidate.

## Blocking Finding

### F1-B1: Registered workers never become schedulable

- `crates/controller/src/workers/service.rs:24-42` creates every worker offline.
- `crates/controller/src/operations/workers.rs:39-50` returns the newly created offline record and does not probe health or capabilities.
- `crates/controller/src/remote/catalog.rs:53-76` implements live workflow/preset capability discovery, but production source has no caller.
- `crates/controller/src/remote/cache.rs:58-116` implements the required TTL/invalidation cache, but production source has no caller.
- `crates/controller/src/recovery/worker.rs:10-60` only persists failed health checks for workers already assigned to tasks; it preserves existing capabilities and never performs successful discovery.
- `crates/controller/src/persistence/reservation.rs:19-32` requires the worker to be online and its persisted capability JSON to contain the task workflow.
- Test fixtures such as `crates/controller/tests/task20/support/controller.rs:163-174` and `crates/controller/tests/task11/support.rs:110-119` directly inject online/capability state, masking the absent production loop.

Required correction: add a production worker health/capability refresh service that periodically probes enabled workers, calls typed health and capability discovery, persists successful and failed `WorkerHealthUpdate` records with bounded retry/backoff, invalidates/reuses the capability cache according to the locked contract, wakes scheduling after durable updates, and is started/stopped by the Controller runtime. Prove normal API/UI worker registration reaches online/compatible scheduling without direct database or fixture writes.

## Tasks 1-25 Compliance Map

| Task | Result | Audit basis |
|---:|---|---|
| 1 | PASS | Isolated `videnoa-controller` workspace member/binary, independent frontend build/embed/debug serving, typed missing-assets failures, and GPU/core-free dependency tree are implemented and tested. |
| 2 | PASS | Typed IDs/DTOs/enums/config, strict unknown-key and bounds validation, stable snake_case wire values, exact path/extension preservation, and locked defaults are implemented and tested. |
| 3 | PASS | SQLite migrations, WAL/foreign keys/busy timeout/bounded pool, repositories, constraints, indexes, CAS transitions, atomic reservations, and 20,000-row query plans are implemented and tested. |
| 4 | PASS | Single-admin Argon2id auth, digest-only sessions, Bearer/session boundaries, CSRF/same-origin enforcement, throttling, rotation/expiry, redaction, and descriptor-rooted path capabilities are implemented and tested. |
| 5 | PASS | Videnoa `POST /api/run` durable keyed create/replay/conflict/concurrency/restart behavior is additive and backward compatible; focused core idempotency suite passed 19/19. Worker database loss is surfaced to Controller as missing/ambiguous remote evidence rather than automatic resubmission. |
| 6 | PASS | Wire-level mock Videnoa and deterministic faults are confined to integration-test modules; production manifest/source has no mock dependency or fixture path. |
| 7 | PASS | Authenticated idempotent `POST /api/tasks`, bounded list/detail pagination, allowlisted filters/sorts/search, rooted immutable paths, collision rejection, and concurrent duplicate protection are implemented and tested at 20,000 rows. |
| 8 | **FAIL** | Typed client, streaming, status taxonomy, workflow/preset merging, exact remote paths, and cache type exist, but production never calls capability discovery or uses the cache. |
| 9 | PASS | Exhaustive lifecycle/recovery/cancellation tables, durable pre-side-effect transitions, bounded retries, explicit processing retry, downstream resume, and non-retryable ambiguity are implemented and tested. |
| 10 | **FAIL** | Task-stage restart reconciliation, outage retention, ambiguity, and graceful shutdown are implemented, but startup/runtime recovery has no general worker health/capability refresh responsibility. |
| 11 | **FAIL** | Registry CRUD, atomic capacity, priority, prefetch, pause, disable/delete policy, and independent transfer pools exist, but ordinary registered workers remain offline/incompatible forever without test-only state injection. |
| 12 | PASS | Restart-safe streamed upload/download, exact stat/length/hash handling, partial cleanup, retry-from-zero, independent limits, and downstream no-compute-replay behavior are implemented and tested. |
| 13 | PASS | Descriptor-rooted hidden staging, hash/length evidence, platform no-replace finalization, publication ambiguity, local cleanup, mandatory remote cleanup, and 404 convergence are implemented and tested. |
| 14 | PASS | Authenticated worker/settings/control APIs, optimistic versions, readiness, counts, cancellation/retry, bounded SSE capacity, active deltas, passive auth recheck, and lag/refetch behavior are implemented and tested. |
| 15 | PASS | Authenticated same-origin shell, login/logout/session bootstrap, in-memory CSRF, no credential storage, protected routes, focus behavior, and SPA routing are implemented and browser-tested. |
| 16 | PASS | Dense server-paginated Tasks table, URL query state, compact counters, allowed filters/sorts/search, bounded pages, and active-row SSE merge/refetch behavior are implemented and tested with 20,000 rows. |
| 17 | PASS | Manual creation uses the same task endpoint with retry-stable idempotency, exact paths and manual source; detail/attempt/error/progress panes plus stage-aware cancel/retry behavior are implemented and tested. |
| 18 | PASS | Compact Workers and Settings pages expose required operational state/actions, optimistic conflicts, pause semantics, transfer/prefetch/timeouts/retries, and read-only root/auth status without browser-editable secrets. |
| 19 | PASS | Chromium E2E covers auth, task workflows, workers/settings, SSE, errors, axe checks, keyboard/focus, reduced motion, forced colors, desktop/narrow/CJK containment, and empty browser credential stores; 43/43 passed. |
| 20 | PASS | Real Controller HTTP plus one/three mock workers proves complete pipelines, remote/local crash checkpoints, outages, cancellation, retry, pause, cleanup, retained history, and one remote job per attempt. Evidence exists under `task-20/happy/` and `task-20/fault-matrix/`. These tests inject worker capabilities and therefore do not cure F1-B1. |
| 21 | PASS | 20,000-row load, snapshot-consistent attempts, repeated intake/reservation/publication races, path/auth/CORS/SSE attacks, no-clobber filesystem cases, independent pools, secret/browser-state checks, and visual evidence are present and pass. |
| 22 | PASS | Dedicated Debian GPU-free image runs as `10001:10001`, embeds the SPA, persists data/temp/config/NAS mounts, exposes health, contains no GPU/model/core runtime, and passes live happy/error smoke. |
| 23 | PASS | Deterministic Linux and Windows scripts enforce exact names/root members/version/content. Linux packaging passed live; Windows PE/archive execution is correctly assigned to `windows-latest` and locally covered by static contract tests. |
| 24 | PASS | CI includes Controller Rust/web/fault/load/container/Linux/Windows jobs while preserving legacy jobs. Release graph preserves existing Videnoa assets/tags and adds exact Controller archives and `controlnet/videnoa-controller:<version|latest>` using existing credentials. Hosted publication remains a truthful host boundary. |
| 25 | PASS | Root/archive/operator documentation and example config cover exact intake modes, auth, roots, workers, lifecycle, retry/cancel, ambiguity, persistence, backup/restore, migration/rollback, Docker/archives, API/SSE, release names, non-goals, and troubleshooting; docs contracts pass. |

## Scope and Must-NOT-Have Audit

| Constraint | Result | Evidence |
|---|---|---|
| Preserve `videnoa` and `videnoa-desktop` | PASS | Existing package/binary names remain; Controller is additive. Legacy release assets and `controlnet/videnoa` tags remain in the release graph. |
| Controller must not depend on GPU/core runtime | PASS | Controller manifest/tree contains no `videnoa-core`, ORT, CUDA, cuDNN, TensorRT, or model runtime. Container/archive scans also reject them. |
| Exactly two intake modes | PASS | Production exposes generic authenticated `POST /api/tasks`; the manual UI calls the same endpoint. No other discovery/intake service exists. |
| No watchers/polling discovery/ANI-RSS/qBittorrent/cron/rules engine | PASS | No production implementation or dependency found. ANI-RSS appears only as opaque informational `source_reference` test/documentation data. |
| No SSH/SFTP/rsync/object storage/broker/consensus/Kubernetes/GPU scheduling | PASS | No matching production modules/dependencies/routes; workers are HTTP Videnoa instances with compute-slot policy only. |
| SQLite authority; runtime signals are ephemeral | PASS | All task/attempt/worker/settings/session/idempotency authority is persisted. Broadcasts only wake orchestration or carry bounded UI invalidation/deltas. |
| Permanent bounded history and bounded realtime | PASS | Task and attempt APIs are paginated with maximum limits; SSE channel capacity is 64 and lag/reconnect produces a refetch event rather than history replay. |
| No blind AI resubmission | PASS | Submission keys are persisted before remote calls and replayed idempotently; missing/contradictory evidence becomes non-retryable ambiguity; downstream retries preserve the compute attempt. |
| No permissive CORS or browser-to-worker calls | PASS | Browser client is same-origin Controller-only; security tests prove cross-origin preflight receives no CORS authorization. |
| No secret persistence/log/browser storage | PASS | Server stores digests/fingerprints, cookie is HttpOnly/Strict, CSRF is memory-only, and tests/evidence scan storage and responses. |
| No unsafe NAS paths or output overwrite | PASS | Capability-rooted no-follow opens and rechecks guard input/output. Downloads remain in temp; publication uses task-owned hidden staging and no-replace finalization. |
| No automatic history deletion | PASS | No task/attempt deletion route or retention job exists; terminal rows remain queryable. |
| No unrelated Controller scope | PASS | Diff against the correct Controller baseline contains only Controller work plus the narrowly required additive Videnoa `/api/run` idempotency change and workspace/cache enumeration. Unrelated earlier branch history is outside this implementation range. |

## Verification Performed

- `git rev-parse HEAD`, `git rev-parse origin/dev`, and `git rev-list --left-right --count HEAD...origin/dev`: identical `ca0b27e`, `0 0`.
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy -p videnoa-controller --all-targets --all-features -- -D warnings`: PASS.
- `cargo test -p videnoa-controller --all-targets`: PASS, including Task 20/21 fault, load, concurrency, filesystem, resource, security, and operations suites.
- `cargo test -p videnoa-core --lib --tests idempotency`: PASS, 19 focused idempotency tests.
- `cargo tree -p videnoa-controller --edges normal`: PASS; no core/GPU/model dependency path.
- `npm ci --no-fund`: PASS, zero reported vulnerabilities.
- `npm run lint`: PASS.
- `npm test -- --run`: PASS, 104/104.
- `npm run build`: PASS.
- `npm run test:e2e`: PASS, 43/43 Chromium scenarios.
- `bash scripts/tests/package_controller_test.sh`: PASS.
- `bash scripts/tests/controller_archive_root_files_test.sh`: PASS.
- `bash scripts/tests/package_controller_windows_static_test.sh`: PASS.
- `bash scripts/tests/controller_docs_test.sh`: PASS.
- `node --test scripts/tests/validate_ci_release_workflows.test.mjs`: PASS, including all negative mutations.
- `bash scripts/check_controller_container.sh videnoa-controller:qa --all`: PASS for source, image content/linkage/user, live health/SPA/persistence, and configured failure paths.

## Audit Boundaries

- Native Windows PE/archive creation is a `windows-latest` responsibility; the Linux audit verified the PowerShell contract and CI wiring without claiming native Windows execution.
- GitHub Release creation and Docker Hub publication are hosted side effects; the audit verified parsed workflow graph, exact assets/tags, credentials convention, failure dependencies, and post-publication checks without creating a release.
- The rejected verdict is independent of these truthful host boundaries. F1-B1 is present in production source and blocks the normal operator workflow on every platform.

VERDICT: REJECT
