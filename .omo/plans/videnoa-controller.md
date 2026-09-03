# videnoa-controller - Work Plan

## TL;DR (For humans)

- **What you will get:** A new independently runnable, GPU-free `videnoa-controller` for NAS deployment, with SQLite-backed task history, authenticated API/Web UI, multi-Videnoa scheduling, streamed transfers, crash-safe reconciliation, no-clobber publishing, cleanup, and independent Docker/GitHub releases.
- **Why this approach:** SQLite and explicit stage transitions keep the internal system understandable while surviving long jobs and restarts. Durable idempotency at both Controller intake and Videnoa `/api/run` closes duplicate-delivery/submit crash windows that polling alone cannot close.
- **What it will not do:** It will not rename or absorb `videnoa`, depend on GPU code, watch folders, integrate ANI-RSS/qBittorrent semantics, distribute workflows, schedule GPUs directly, introduce brokers/distributed consensus, overwrite media, or rerun AI after downstream failures.
- **Shape:** 4 implementation waves, **25 implementation todos**, followed by **4 parallel final-verification tasks** that must all approve.
- **Effort / risk:** Architecture-scale. Highest-risk areas are remote submit idempotency, filesystem publication crash recovery, path/auth boundaries, and preserving existing CI/release products; each has dedicated TDD, fault injection, and final audit coverage.
- **Locked owner decisions:** One admin password for Web login and Bearer API access; canonical NAS root allowlists; output no-clobber; TDD; `controlnet/videnoa-controller` image plus independent Linux/Windows archives.
- **Added from repository analysis:** Durable `Idempotency-Key` contracts, destination-side hash-backed publication recovery, explicit cancellation/ambiguity rules, indexed 20,000-row load proof, worker persistent-data requirement, auth/CSRF/rate-limit hardening, and migration/rollback verification.

## Scope

### In

- Add the GPU-free Cargo workspace member `crates/controller/` and independently runnable `videnoa-controller` binary. Keep the existing `videnoa` and `videnoa-desktop` product names, dependency graph, archive layouts, and GPU image intact.
- Add a separate `controller-web/` React 19 + TypeScript + Vite + Tailwind frontend, embedded into Controller release builds and served from disk in debug builds, following `crates/core/build.rs:10-80` and `crates/core/src/server/mod.rs:542-569` without coupling Controller to `videnoa-core`.
- Implement one same-origin Controller API for manual GUI and external intake. Both use authenticated `POST /api/tasks`; `source` is informational only. Do not add ANI-RSS semantics.
- Persist tasks, attempts, workers, scheduler pause, sessions, task-ingress idempotency, progress, errors, assignments, and remote job identifiers in SQLite. SQLite is authoritative; channels and broadcasts only wake runtime loops or deliver ephemeral UI deltas.
- Schedule named compatible workflows across registered Videnoa service instances with configurable `compute_slots`, `prefetch_per_worker`, and independent upload/download concurrency limits.
- Transfer files only through `/api/files/*`; execute and poll through Videnoa HTTP APIs. NAS paths remain Controller-local and are restricted to configured input/output root capabilities.
- Implement stage-aware restart reconciliation, cancellation, bounded transient retries, no-duplicate remote submission, safe download verification, no-clobber publication, local temp cleanup, and mandatory remote workspace cleanup.
- Retain task/attempt history indefinitely and expose indexed server-side pagination, filtering, search, sorting, and active-row SSE updates suitable for tens of thousands of tasks.
- Deliver dense Tasks, Workers, and Settings pages; independent Linux/Windows archives; `controlnet/videnoa-controller` Docker tags; CI/release checks; configuration and operator documentation.

### Out / Must NOT Have

- No rename of the existing Videnoa application; no `videnoa-controller -> videnoa-core` dependency; no ONNX Runtime, CUDA, cuDNN, TensorRT, models, or GPU runtime in Controller artifacts.
- No filesystem watchers, directory polling, ANI-RSS/qBittorrent adapters, cron discovery, rules engine, workflow deployment/synchronization, media-browser expansion, or Jellyfin refresh integration.
- No SSH, SFTP, rsync, NFS/SMB/remote mounts, S3/object storage, resumable-transfer protocol, Redis/PostgreSQL requirement, brokers, consensus, Kubernetes scheduler, GPU-ID/VRAM/device scheduling, or generic event-sourcing subsystem.
- No multiple users, roles, accounts, or ACL system. One admin password only; never persist or log its plaintext or retain it in browser storage.
- No permissive Controller CORS, direct browser-to-Videnoa calls, unbounded task listing, card-heavy task history, whole-table realtime replacement, queue-wide pre-upload, or shared upload/download semaphore.
- No guessed/normalized output extension, mutation/deletion of `input_path`, silent output overwrite/rename, direct download into the final media directory, or deletion of an ambiguous/unrelated final file.
- No blind AI resubmission after timeout, crash, missing evidence, worker database loss, or later-stage failure. Download, verify, publish, and cleanup retries never repeat completed compute.
- No automatic deletion of completed Controller task or attempt history; no change to existing Videnoa archive/image layouts.

## Verification strategy

### Test strategy

- **TDD is mandatory.** Every implementation todo starts by adding a failing unit/integration/UI test, records the red result, adds the minimal implementation, then records the green result and relevant regression suite.
- Test-only remote behavior lives under `crates/controller/tests/support/`; production code must contain no dummy workers, fake success paths, or test credentials.
- Evidence goes under `.omo/evidence/videnoa-controller/task-<N>/` with command logs, JSON responses, database assertions, screenshots, and fault-injection traces. Never include the real admin password, Authorization headers, cookies, or CSRF values.

### Standard commands

Controller Rust gates:

```bash
cargo fmt --all -- --check
cargo clippy -p videnoa-controller --all-targets --all-features -- -D warnings
cargo test -p videnoa-controller --all-targets
cargo tree -p videnoa-controller -i videnoa-core
```

Expected: formatting/clippy/tests exit `0`; the inverse dependency query reports that `videnoa-core` does not depend on Controller and a direct tree inspection confirms Controller has no path to `videnoa-core`, `ort`, CUDA, cuDNN, or TensorRT.

Existing Videnoa regression gates, with the repository runtime environment:

```bash
export ORT_DYLIB_PATH="$PWD/lib/libonnxruntime.so"
export TRT_LIBS="$HOME/miniconda3/envs/anime/lib/python3.13/site-packages/tensorrt_libs"
export LD_LIBRARY_PATH="$TRT_LIBS:$PWD/lib:$HOME/miniconda3/envs/anime/lib:${LD_LIBRARY_PATH:-}"
export PKG_CONFIG_PATH="$HOME/miniconda3/envs/anime/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
cargo test -p videnoa-core --lib --tests
cargo test --workspace
```

Expected: all tests exit `0`, including the additive Videnoa run-idempotency migration/API tests.

Controller frontend gates:

```bash
cd controller-web
npm ci --no-fund
npm run lint
npm test -- --run
npm run build
npm run test:e2e
```

Expected: lint, Vitest, production build, and Playwright exit `0`; screenshots show a dense table at desktop width and usable controls/detail panes at a narrow viewport.

Packaging gates:

```bash
docker build -f Dockerfile.controller -t videnoa-controller:qa .
docker run --rm videnoa-controller:qa videnoa-controller --help
bash scripts/package_controller.sh --target x86_64-unknown-linux-gnu
pwsh -File scripts/package_controller.ps1 -Target x86_64-pc-windows-msvc
```

Expected: the image starts without GPU flags/libraries; archives have the locked layouts and contain no ONNX/CUDA/TensorRT/model files; existing Videnoa packaging smoke tests still pass.

### Required fault matrix

- Restart Controller before/after reservation, mid-upload, after upload, before remote submit, after remote acceptance but before response/local job persistence, during poll, after remote completion, mid-download, after download sync, before/after destination staging, after final no-clobber rename but before SQLite update, during local cleanup, and during remote cleanup.
- Restart Videnoa while queued/running and simulate health, upload, submit, poll, download, and cleanup outages.
- Assert one remote job per attempt idempotency key; no partial final file; no overwrite; no compute replay for downstream failures; no leaked capacity; and persistent actionable failure when evidence is ambiguous.
- Seed at least 20,000 task rows and verify stable pages, filters, allowed sorts, indexed query plans, bounded API payloads, and active-row-only SSE updates.
- Exercise authentication, session expiry/logout/rotation, CSRF, login throttling, CORS absence, secret/log redaction, path traversal/symlink/race rejection, and authenticated SSE.

## Execution strategy

### Locked contracts

- Controller API uses JSON under `/api`. Public liveness `GET /api/health` is unauthenticated; readiness, all task/worker/settings routes, SSE, and logout require either a valid session or `Authorization: Bearer <admin-password>`. Login accepts the password and issues the session cookie.
- `POST /api/tasks` requires `Idempotency-Key`. First use returns `201`; replay with the same canonical request hash returns the original task (`200`); reuse with different content returns `409`. The GUI creates one key per submit action and reuses it across network retries.
- Add the same `Idempotency-Key` behavior to Videnoa `POST /api/run`, backed by durable job persistence. Same key/same workflow+params returns the existing job; changed payload returns `409`; every new Controller compute attempt gets a new UUID key persisted before the request.
- Require persistent Videnoa `data/` storage for crash-safe reconciliation. Missing/contradictory remote evidence or lost Videnoa job DB becomes non-retryable `remote_state_ambiguous`; never resubmit automatically.
- Task order is `priority DESC, created_at ASC, id ASC`. Worker selection filters enabled/online/workflow-compatible instances, then chooses available capacity by `used_slots ASC, last_assigned_at ASC, id ASC`. Uploads that feed an idle worker outrank optional prefetch.
- Default settings: `compute_slots=1`, `prefetch_per_worker=1`, `max_concurrent_uploads=1`, `max_concurrent_downloads=1`. Scheduler pause is persisted; it blocks new reservation, prefetch/upload, and compute submission, preserves staged reservations, and allows poll/download/verify/publish/cleanup to continue.
- Immutable paths: task creation requires an existing regular input under an input root and a non-existing output whose nearest existing parent is under an output root. Snapshot input size/mtime; re-open through root capability and re-check before upload. Output collision at creation or publication fails no-clobber; a different output requires a new task.
- Remote workspace remains `<task-id>/input.<input-ext>` and `<task-id>/output.<output-ext>`. A new compute attempt may start only after the prior job is terminal and the old workspace is confirmed cleaned.
- Download to `<data-dir>/temp/<task-id>/output.<ext>.part`, stream SHA-256 and length, sync, then rename within temp. Publish through a hidden task-owned file in the destination directory, sync it, and finalize with platform no-replace semantics: Linux `renameat2(RENAME_NOREPLACE)` through `rustix`; Windows `MoveFileExW` without replace flags through `windows-sys`. Unsupported platforms fail safely rather than falling back to overwriting rename.
- Before finalization persist expected length, SHA-256, and hidden staging name. On `publishing` recovery: an exact final hash/length match advances to cleanup; a valid staging file resumes no-replace finalization; mismatch/unknown ownership becomes non-retryable `publication_ambiguous` without deleting either file.
- Cancellation is accepted through `verifying`: persist `cancel_requested_at` first, abort local work, reconcile `submitting`, cancel active remote jobs only after Controller evidence is durable, and clean task-owned files. `publishing`, `remote_cleanup`, and terminal tasks return `409` because irreversible publication/cleanup must converge.
- Transient upload/download/health/cleanup retries use bounded exponential backoff with jitter and persist their next-attempt metadata. Processing failures require an explicit retry that closes the old attempt, cleans its workspace, creates a new attempt/submission key, and may select another worker. Ambiguous remote/publication failures cannot be retried.
- SSE is ephemeral and carries active task/worker deltas only; database state remains authoritative. A lagged/reconnected client refetches its current page and active counters.

### Waves and dependencies

| Wave | Todos | Outcome | Depends on |
|---|---:|---|---|
| 1 — Contracts and foundations | 1-6 | GPU-free product shell, locked domain/API contracts, durable schema, auth/path primitives, remote idempotency, test fixture | none |
| 2 — Durable orchestration | 7-13 | Intake/history APIs, typed remote client, state machine, reconciliation, scheduler, transfers, safe publication/cleanup | Wave 1 |
| 3 — Operations API and GUI | 14-19 | Worker/settings/control/SSE APIs and complete dense authenticated GUI | Waves 1-2 |
| 4 — Delivery and system proof | 20-25 | End-to-end fault/load/security proof, independent container/archives, CI/release, docs | Waves 1-3 |

- Within a wave, todos marked parallel may run concurrently only after their stated dependencies pass. Database/domain contracts from Todos 2-4 and remote idempotency from Todo 5 are gates for orchestration work.
- Keep production Rust modules at or below 250 pure LOC; split repositories, routes, and stage handlers before crossing the limit. Use typed errors and exhaustive state matches; no `any`, unchecked `unwrap`, or panic-based request handling.
- Before each implementation commit, inspect the worktree and stage only task-owned paths. Never use `git clean`, destructive reset/checkout, or force push. The repository completion rules require pull/rebase, push, and final up-to-date status after all verification.

## Todos

### Wave 1 — Contracts and foundations

- [x] 1. Add the isolated Controller workspace member and frontend build shell

  **Depends on:** none

  **References:** `Cargo.toml:1-35`; `crates/core/Cargo.toml:6-35`; `crates/core/build.rs:10-80`; `crates/core/src/server/mod.rs:542-569`; `web/package.json:6-59`; `Dockerfile:39-62`.

  **Work:** First add a failing workspace/build contract test that expects a `videnoa-controller` package, binary, and independent frontend asset directory. Then add `crates/controller/Cargo.toml`, minimal `src/lib.rs`/`src/main.rs`, `build.rs`, and root `controller-web/` using the existing npm/Vite/React/Tailwind versions and conventions. Release builds run `npm ci --no-fund && npm run build` and embed `controller-web/dist`; debug serves that directory with SPA fallback. Add the workspace member and update Docker dependency-cache manifest enumeration without changing existing binary targets. Keep Controller direct dependencies GPU-free.

  **Acceptance:** `cargo metadata` lists `videnoa-controller`; `cargo build -p videnoa-controller` does not invoke/link GPU libraries; release build embeds a generated GUI; debug mode serves the same routes from disk; existing crates remain named and buildable.

  **QA:** Happy—run the Controller build and request `/` plus `/api/health`, saving responses to `.omo/evidence/videnoa-controller/task-1/health-and-spa.txt`. Failure—temporarily omit `controller-web/dist` in a test fixture and assert debug startup reports a typed missing-assets error while release embedding tests remain deterministic; save `.omo/evidence/videnoa-controller/task-1/missing-assets.txt`.

  **Commit:** `feat(controller): add isolated service and web build shell`

- [x] 2. Lock typed domain, configuration, and HTTP contracts

  **Depends on:** 1

  **References:** `.omo/knowledges/videnoa-controller-request.md:5-32`; `.omo/drafts/videnoa-controller.md:47-63`; `crates/core/src/server/mod.rs:282-306,420-450`.

  **Work:** Start with failing serialization/config tests. Add focused modules under `crates/controller/src/domain/` and `config.rs` for branded IDs, `Task`, `TaskAttempt`, `Worker`, lifecycle/failure enums, progress, page/filter/sort DTOs, worker/settings DTOs, and typed API errors. Lock lifecycle states to `queued`, `reserved`, `uploading`, `staged`, `submitting`, `processing`, `remote_completed`, `downloading`, `verifying`, `publishing`, `remote_cleanup`, `completed`, `failed`, `cancelled`; use `failure_stage`, stable `failure_code`, `retryable`, and `cancel_requested_at` for exceptional outcomes. Lock `POST /api/tasks`, task detail/list, worker, settings, auth, cancel/retry, SSE, health/readiness schemas. Add config sections for server/data/temp roots, auth hash file/session defaults, scheduler defaults, health/poll/transfer timeouts, retry bounds, and environment overrides. Reject unknown config keys and invalid bounds.

  **Acceptance:** Every persisted/API enum round-trips with stable snake_case values; exhaustive matches compile; task request preserves exact path strings/extensions; defaults match the locked contracts; invalid roots, zero slots, zero/max-overflow page limits, malformed URLs, and missing password-hash file fail startup with typed messages.

  **QA:** Happy—load a complete test config and snapshot all public JSON DTOs at `.omo/evidence/videnoa-controller/task-2/contracts.json`. Failure—run table tests for unknown keys, invalid timeout/slot/root combinations, and unknown enum values; save `.omo/evidence/videnoa-controller/task-2/config-errors.txt`.

  **Commit:** `feat(controller): define domain and configuration contracts`

- [x] 3. Add SQLite migrations, repositories, indexes, and atomic transition primitives

  **Depends on:** 2

  **References:** `.omo/knowledges/videnoa-controller-request.md:19-38`; `.omo/knowledges/videnoa-controller-repo-findings.md:34-39`; `crates/core/src/server/persistence.rs:64-216`.

  **Work:** Begin with failing migration/repository tests using temporary databases. Use SQLx SQLite with explicit migrations, WAL, foreign keys, busy timeout, bounded connections, and blocking-safe async access. Create exact tables: `tasks`, `task_attempts`, `workers`, single-row `controller_settings`, `auth_sessions`, and `task_idempotency`; rely on SQLx migration metadata rather than inventing a second framework. Include durable version/CAS columns, retry timestamps/counts, progress, expected output size/SHA-256, destination staging name, input snapshot, remote paths/job/submission key, and all lifecycle timestamps. Add uniqueness for `(task_id, attempt_no)`, remote submission key, task idempotency key, worker name, and normalized API URL. Add indexes for status/created, completed, worker, workflow, source, failure stage, priority queue order, retry wakeups, sessions, and stable list sorts. Implement conditional transitions, atomic reservation/attempt creation, slot-count queries, page/count queries, and startup recovery queries.

  **Acceptance:** Migrations apply to empty DB, are idempotent through SQLx, and fail atomically on invalid schema; foreign keys and constraints reject corrupt rows; concurrent reservations cannot claim one task twice or exceed worker capacity; 20,000-row query plans use intended indexes and return stable pages.

  **QA:** Happy—run repository/concurrency/migration tests and save schema plus `EXPLAIN QUERY PLAN` output to `.omo/evidence/videnoa-controller/task-3/schema-and-indexes.txt`. Failure—inject migration failure and conflicting transition/reservation writes; assert rollback/no double claim and save `.omo/evidence/videnoa-controller/task-3/atomic-failures.txt`.

  **Commit:** `feat(controller): add durable sqlite state store`

- [x] 4. Implement single-admin authentication, sessions, CSRF, redaction, and NAS path capabilities

  **Depends on:** 2, 3

  **References:** `.omo/drafts/videnoa-controller.md:54-57`; `.omo/knowledges/videnoa-controller-request.md:51-57`; `crates/core/src/server/files/path.rs:22-191`; `crates/core/src/server/tests/files/security.rs:3-89`; `crates/core/src/server/mod.rs:577-624`.

  **Work:** Add failing auth and path-escape tests first. Implement `videnoa-controller hash-password` producing an Argon2id PHC string; runtime reads only a configured hash file. Web login verifies the raw admin password and creates a random 256-bit server-side session whose token digest, expiry, idle deadline, CSRF digest, and password-hash fingerprint are stored in SQLite. Default absolute/idle lifetimes are 24h/1h; logout, expiry, and hash-file rotation invalidate sessions. External clients use the same raw password via `Authorization: Bearer`; compare safely against the Argon2id hash. Apply login throttling of 5 failures per IP per 5 minutes with `429`, constant response shape, and no credential reflection. Cookie is `HttpOnly`, `SameSite=Strict`, path `/`; `Secure=true` by default with an explicit warned HTTP-only override. Cookie mutations require same-origin plus CSRF header; Bearer requests are CSRF-exempt. Do not install permissive CORS; redact auth/cookie/CSRF fields from errors/logs.

  Implement root-confined filesystem access with capability-style handles (using `cap-std` or an equivalently no-follow descriptor-based API): inputs must resolve to existing regular files below configured input roots; outputs map to a non-existing leaf whose nearest existing parent is below an output root. Reject symlink roots/components, traversal, malformed drive/UNC paths, root changes that invalidate queued tasks, and local interpretation of remote workflow paths containing `..`. Re-open/re-check input identity/size/mtime before upload and output parent/no-clobber immediately before publication.

  **Acceptance:** Only health/login are public; readiness, SSE, and operational routes enforce session or Bearer auth. No raw secret enters SQLite/config/logs/browser storage. CSRF/session/rate-limit/rotation behavior matches the contract. All local reads/writes are rooted and race-resistant; outside-root/symlink paths never reach transfer/publication code.

  **QA:** Happy—exercise login, session, Bearer, logout, expiry/rotation, valid rooted input/output, and save sanitized HTTP traces to `.omo/evidence/videnoa-controller/task-4/auth-path-happy.txt`. Failure—test wrong/missing auth, sixth failed login, missing CSRF, cross-origin, traversal, symlink swap, output collision, and malformed Windows paths; scan captured logs for the test secret and save `.omo/evidence/videnoa-controller/task-4/auth-path-failures.txt`.

  **Commit:** `feat(controller): secure access and filesystem boundaries`

- [x] 5. Add durable idempotency to Videnoa remote job submission

  **Depends on:** 2

  **References:** `crates/core/src/server/mod.rs:420-450,1252-1407,1498-1519,1558-1604,2568-2582`; `crates/core/src/server/persistence.rs:64-216`; `.omo/knowledges/videnoa-controller-repo-findings.md:10-18`.

  **Work:** First add failing Videnoa API/persistence tests for lost-response replay and key collision. Extend `POST /api/run` to accept an optional standard `Idempotency-Key` header without breaking clients that omit it. Persist the key and a canonical SHA-256 fingerprint of workflow name plus params in the jobs table under a unique constraint before queueing execution. First keyed submission returns `201`; same key/fingerprint returns the existing job and `200`; same key/different fingerprint returns `409`; concurrent duplicates create one job. Preserve keys/status across restart. Keep existing unkeyed behavior and JSON request compatibility. Document that Controller deployments require persistent worker `data/`; database loss yields ambiguous state rather than exactly-once guarantees.

  **Acceptance:** A timeout/replay around accepted submission cannot create a second Videnoa job; migration preserves existing rows; old clients/tests continue to work; keyed job lookup remains possible after process restart; cancelled-on-restart behavior stays unchanged and visible to Controller.

  **QA:** Happy—submit the same keyed request sequentially/concurrently and after Videnoa restart; assert one UUID and save DB/API evidence to `.omo/evidence/videnoa-controller/task-5/idempotent-run.txt`. Failure—reuse the key with changed params and simulate migration conflict/database loss; assert `409` or explicit ambiguous-state guidance, saving `.omo/evidence/videnoa-controller/task-5/idempotency-failures.txt`.

  **Commit:** `feat(server): make run submission idempotent`

- [x] 6. Build the test-only mock Videnoa and deterministic fault-injection harness

  **Depends on:** 1, 2, 5

  **References:** `crates/core/src/server/mod.rs:577-624`; `crates/core/src/server/files.rs:34-177`; `crates/core/src/server/tests/files/lifecycle.rs:12-122`; `.omo/knowledges/videnoa-controller-repo-findings.md:10-26`.

  **Work:** Add a test-only Axum server under `crates/controller/tests/support/` implementing health, workflows, presets, interface lookup, idempotent run, job status/cancel, and file PUT/GET/stat/DELETE. Give tests deterministic controls for disconnect-before-accept, accept-then-drop-response, restart-cancelled jobs, offline periods, truncated streams, delayed polls, DELETE 404/5xx, corrupt output, and request counters. Store mock state durably when a restart test requests it. Never compile this harness into production targets.

  **Acceptance:** Integration tests can prove exact remote request counts and bytes, pause/resume faults at named boundaries, restart the mock while retaining/loss-testing DB state, and assert cleanup. Production dependency/tree and binary-symbol checks find no mock fixture.

  **QA:** Happy—run one complete mocked upload/run/poll/download/delete sequence and save the request journal to `.omo/evidence/videnoa-controller/task-6/mock-happy.json`. Failure—trigger each fault mode and assert deterministic typed outcomes, saving `.omo/evidence/videnoa-controller/task-6/mock-faults.txt`.

  **Commit:** `test(controller): add remote fault injection harness`

### Wave 2 — Durable orchestration

- [x] 7. Implement idempotent task intake and indexed history APIs

  **Depends on:** 3, 4

  **References:** `.omo/knowledges/videnoa-controller-request.md:12-17,34-38`; `.omo/drafts/videnoa-controller.md:60-61`; `crates/core/src/server/mod.rs:1498-1505` as the unbounded pattern not to copy.

  **Work:** Add failing API/repository tests, then implement authenticated `POST /api/tasks`, `GET /api/tasks`, and `GET /api/tasks/{id}`. Require `Idempotency-Key`; canonicalize the JSON request for a durable fingerprint; same key/body returns the original row, changed body returns `409`, and concurrent duplicates insert once. Validate immutable input/output semantics, workflow/source/priority bounds, input snapshot, roots, and output non-existence. Use offset pagination with default `limit=100`, maximum `500`, `offset>=0`; stable tie-break by task ID. Support allowed filters for status, worker, workflow, source, failure stage, and case-insensitive basename/path search; allowlisted sorts `priority`, `created_at`, `completed_at`, `status`, `worker`, `duration`, each with deterministic ID tie-break. Return `items,total,limit,offset` without loading all rows.

  **Acceptance:** Manual/external callers receive the same task representation; duplicate delivery creates one task; output collision and invalid paths fail before queueing; list count/page/filter/sort use indexed repository methods and stay bounded.

  **QA:** Happy—create/replay tasks and traverse filtered pages from a 20,000-row seed, saving responses/query plans to `.omo/evidence/videnoa-controller/task-7/history-api.txt`. Failure—changed-body key reuse, concurrent duplicates, outside roots, changed input snapshot, invalid sort/filter/limit, and existing output return stable 4xx errors; save `.omo/evidence/videnoa-controller/task-7/history-errors.txt`.

  **Commit:** `feat(controller): add task intake and history api`

- [x] 8. Implement the typed Videnoa client and workflow compatibility cache

  **Depends on:** 2, 4, 5, 6

  **References:** `crates/core/src/server/mod.rs:420-450,1252-1923`; `crates/core/src/server/files.rs:34-177`; `crates/core/src/server/tests/files/mod.rs:65-84`; `presets/anime-2x-upscale.json:5-96`.

  **Work:** Begin with mock-backed failing tests. Implement `reqwest` client modules for health, workflows, presets, interface, idempotent run, job polling/cancel, and streaming file PUT/GET/stat/DELETE. Merge workflow and preset names, fetch the interface, and mark a workflow eligible only when it has `Path` inputs exactly named `input` and `output`. Persist the exact upload-returned remote input path and derive only the sibling remote output workflow path; never resolve it locally. Add configured connect/request/stall timeouts, bounded response sizes for JSON, typed status/error parsing, TLS URL support, and log redaction. Cache capabilities with TTL but invalidate on health/restart/error; DB and live remote checks outrank cache.

  **Acceptance:** Client streams files without buffering them wholly, sends idempotency keys, distinguishes 404/409/429/5xx/network/stall errors, handles presets and workflows consistently, and never selects an incompatible worker.

  **QA:** Happy—run compatibility, upload, run, poll, download, and cleanup against the mock, including a returned workflow path containing `..`; save `.omo/evidence/videnoa-controller/task-8/client-happy.txt`. Failure—unknown workflow, missing Path interfaces, oversized/malformed JSON, TLS/network timeout, and status-code matrix map to typed errors; save `.omo/evidence/videnoa-controller/task-8/client-errors.txt`.

  **Commit:** `feat(controller): add typed videnoa http client`

- [x] 9. Implement the exhaustive lifecycle, retry taxonomy, and cancellation matrix

  **Depends on:** 3, 6, 8

  **References:** `.omo/knowledges/videnoa-controller-request.md:19-24`; `.omo/drafts/videnoa-controller.md:49-53`; `crates/core/src/server/mod.rs:1558-1604`; `crates/core/src/server/persistence.rs:156-165`.

  **Work:** Add failing transition-table property tests, then implement one central exhaustive state machine with conditional DB transitions and persist-before-side-effect commands. Encode automatic bounded retries for transient upload/download/health/cleanup failures; explicit processing retry creates a new attempt/key only after terminal evidence and workspace cleanup; later-stage retry resumes that stage. Treat restart-cancelled remote jobs as failed processing attempts requiring explicit retry. Mark missing/contradictory evidence `remote_state_ambiguous` and publication uncertainty `publication_ambiguous`, both non-retryable. Implement cancel rules: queued/reserved cancel locally; uploading/staged abort and clean; submitting reconciles keyed submission before cancellation; processing persists intent then cancels remote and cleans; remote-completed/downloading/verifying abort downstream work and clean without publishing; publishing/remote-cleanup/terminal return `409`. Keep paths immutable; collision requires a new task.

  **Acceptance:** Illegal transitions cannot compile through command dispatch or cannot commit through repository CAS; every nonterminal state has exactly one recovery action; retry/cancel never erases attempt history or repeats successful AI for downstream failures.

  **QA:** Happy—table-test every legal transition, transient retry, explicit new attempt, and cancelable state; save `.omo/evidence/videnoa-controller/task-9/state-matrix.txt`. Failure—exercise every illegal transition, ambiguous evidence, retry-blocked state, and late cancellation; assert stable 409/error codes and save `.omo/evidence/videnoa-controller/task-9/state-errors.txt`.

  **Commit:** `feat(controller): enforce durable task lifecycle`

- [x] 10. Add startup reconciliation, graceful shutdown, and worker-outage recovery

  **Depends on:** 3, 6, 8, 9

  **References:** `crates/core/src/server/persistence.rs:64-216`; `.omo/knowledges/videnoa-controller-repo-findings.md:14-18,34-39`; `.omo/knowledges/videnoa-controller-request.md:19-24`.

  **Work:** Start with restart-boundary failing tests. Add a reconciler separate from the scheduler. On startup, scan every nonterminal task/attempt and dispatch the state-specific recovery action: remote stat for upload, replay same keyed submit, poll known job, restart download from zero, re-verify temp, recover publication from hash/staging evidence, and retry cleanup. Health outages keep assignments/capacity reserved and back off; they never reassign unknown compute. Videnoa restart-cancelled jobs close the attempt as processing failure. Missing worker/job DB evidence becomes actionable ambiguous failure. On SIGINT/SIGTERM, persist pause of new dispatch, stop accepting new stage work, allow a bounded drain of DB writes, leave remote jobs untouched, and exit with recoverable states.

  **Acceptance:** Controller may be killed at every required boundary and restarts into the correct stage; no duplicate job/task/output appears; worker outage does not leak slots or block unrelated workers; shutdown does not cancel remote compute.

  **QA:** Happy—restart at each fault-matrix checkpoint and save task/attempt/remote request journals to `.omo/evidence/videnoa-controller/task-10/recovery-matrix.txt`. Failure—lose remote DB, return contradictory job parameters, and hold a worker offline past retry bounds; assert no resubmit and durable actionable failure in `.omo/evidence/videnoa-controller/task-10/ambiguous-outage.txt`.

  **Commit:** `feat(controller): reconcile durable work after restart`

- [x] 11. Implement worker registry, capacity accounting, scheduler priority, prefetch, and pause

  **Depends on:** 3, 7, 8, 9, 10

  **References:** `.omo/knowledges/videnoa-controller-request.md:26-32`; `.omo/drafts/videnoa-controller.md:50-52,62`; `.omo/knowledges/videnoa-controller-repo-findings.md:16-18`.

  **Work:** Add failing deterministic scheduler/concurrency tests. Implement worker create/update/enable/disable persistence, normalized unique API URLs, health/capability refresh, and atomic capacity accounting. Select tasks by `priority DESC, created_at ASC, id ASC`; select eligible workers by enabled/online/compatible/available then `used_slots ASC, last_assigned_at ASC, id ASC`. Reserve and create attempts atomically. Enforce compute slots, one active upload per worker, configurable prefetch (default one), and independent global upload/download semaphores. Give idle-worker feed uploads priority over optional prefetch. Persist pause in `controller_settings`; pause blocks new reservation/upload/submission, retains staged reservation, and permits poll/download/verify/publish/cleanup. Disabling a busy worker stops new work but keeps reconciliation; deleting a worker with active/history references returns `409` (disable instead).

  **Acceptance:** Thousands of queued tasks remain in SQLite until selected; no capacity/prefetch limit is exceeded under concurrency/restart; incompatible workers receive no task; pause and disable semantics are deterministic and persistent.

  **QA:** Happy—run multi-worker scheduling with mixed workflows/priorities, simultaneous upload/compute/download, idle-feed precedence, pause/restart, and save timeline `.omo/evidence/videnoa-controller/task-11/scheduler-timeline.json`. Failure—race reservations, disable/delete busy worker, saturate pools, and expose incompatible workers; assert no over-allocation/starvation among equal priority in `.omo/evidence/videnoa-controller/task-11/scheduler-failures.txt`.

  **Commit:** `feat(controller): schedule bounded multi-worker pipelines`

- [x] 12. Implement restart-safe upload and download stages with independent limits

  **Depends on:** 6, 8, 9, 10, 11

  **References:** `crates/core/src/server/files.rs:34-145`; `crates/core/src/server/tests/files/lifecycle.rs:12-83`; `.omo/knowledges/videnoa-controller-repo-findings.md:20-26,34-39`.

  **Work:** Add failing truncation/restart/concurrency tests. Upload through a root-confined input handle while recording exact local length; persist `uploading` before PUT. After success or uncertain disconnect, stat the remote file: exact length advances to staged; missing/mismatch deletes the remote partial and retries from zero. Persist the exact returned workflow path. Download only after confirmed remote completion into `<data>/temp/<task-id>/output.<ext>.part`; stream to disk while computing SHA-256/length, enforce non-zero and expected `Content-Length`, flush/sync, then rename within temp to a verified name. On interruption/restart discard `.part` and restart from zero; no range/resumable protocol. Use separate global semaphores and per-worker upload guard so upload, compute, and download overlap.

  **Acceptance:** Partial/mismatched transfers never advance lifecycle; large files are streamed with bounded memory; input/output extensions remain independent; saturated uploads do not block downloads; restart resumes the correct transfer stage without rerunning compute.

  **QA:** Happy—transfer files larger than buffers while compute overlaps and save byte counts/semaphore timeline to `.omo/evidence/videnoa-controller/task-12/transfer-happy.txt`. Failure—truncate/drop/stall PUT and GET, return wrong Content-Length/zero output, kill mid-transfer, and assert partial cleanup/retry-from-zero in `.omo/evidence/videnoa-controller/task-12/transfer-failures.txt`.

  **Commit:** `feat(controller): stream durable upload and download stages`

- [x] 13. Implement verified no-clobber publication and mandatory cleanup convergence

  **Depends on:** 3, 4, 9, 10, 12

  **References:** `.omo/knowledges/videnoa-controller-request.md:34-37`; `.omo/drafts/videnoa-controller.md:56-57,63`; `crates/core/src/server/files.rs:148-177`; `.omo/knowledges/videnoa-controller-repo-findings.md:37-39`.

  **Work:** Start with failing same-filesystem, forced-EXDEV, race, and crash tests. Recheck the output capability and no-clobber immediately before publication. Persist expected hash/length and unique hidden destination staging name before filesystem effects. Copy the verified temp into the hidden file with `create_new`, sync file, then atomically finalize without replacement using Linux `rustix` `RENAME_NOREPLACE` and Windows `MoveFileExW` without replace; sync parent directory where supported. Same-filesystem paths may use the same destination-staging algorithm for one recovery model. On recovery, exact final hash/length proves publication and advances; valid hidden staging resumes; mismatch/unknown ownership fails `publication_ambiguous` and preserves all files. After publication, remove Controller temp, then DELETE the remote task workspace. Treat remote 404 as success, retry network/5xx with persisted backoff, and treat other 4xx as terminal cleanup configuration failure. Mark completed only after both cleanups converge.

  **Acceptance:** Existing/racing destination is never overwritten; no partial artifact appears at `output_path`; crash after final rename converges by hash; unrelated or mismatching final files are untouched; cleanup failures never rerun compute and survive restart.

  **QA:** Happy—publish on same and forced cross-filesystem paths, crash after no-replace finalization, return remote DELETE 404, and save hashes/state transitions to `.omo/evidence/videnoa-controller/task-13/publish-happy.txt`. Failure—pre-create/race destination, corrupt hidden staging/final, deny destination permissions, and return DELETE 400/500; assert preservation and correct retryability in `.omo/evidence/videnoa-controller/task-13/publish-failures.txt`.

  **Commit:** `feat(controller): publish outputs without clobbering`

### Wave 3 — Operations API and dense GUI

- [x] 14. Expose worker, settings, lifecycle-control, readiness, and SSE APIs

  **Depends on:** 4, 7, 9, 10, 11, 13

  **References:** `.omo/knowledges/videnoa-controller-request.md:39-44`; `.omo/drafts/videnoa-controller.md:67-72`; `crates/core/src/server/mod.rs:282-306,577-624`.

  **Work:** Add failing route/authorization tests, then implement authenticated worker CRUD/enable-disable, settings read/update, scheduler pause/resume, task cancel/retry, health/readiness, aggregate status counts, and SSE. Validate optimistic versions on mutable worker/settings rows and return `409` on stale updates. Retry follows Todo 9 and rejects ambiguous/collision/cancelled/completed tasks. Readiness fails when migrations/auth/root handles are invalid but does not require every worker online. SSE emits bounded `task_updated`, `worker_updated`, and `scheduler_updated` active-state deltas; authenticate connections and make lagged/reconnected clients refetch. Never serialize authentication material.

  **Acceptance:** Browser never calls Videnoa directly; all mutations are authenticated/CSRF-safe; APIs use stable typed JSON/status codes; live updates do not resend history; stale writes cannot overwrite newer state.

  **QA:** Happy—exercise worker lifecycle, pause/resume, cancel/retry, readiness, counts, and authenticated SSE; save `.omo/evidence/videnoa-controller/task-14/operations-api.txt`. Failure—test unauthenticated SSE, stale versions, busy-worker deletion, illegal retry/cancel, lagged subscriber, and invalid bounds; save `.omo/evidence/videnoa-controller/task-14/operations-errors.txt`.

  **Commit:** `feat(controller): expose operational control api`

- [x] 15. Build the authenticated Controller frontend shell and design system

  **Depends on:** 1, 4, 14

  **References:** `web/package.json:6-59`; `web/src/`; `.omo/knowledges/videnoa-controller-request.md:39-44`.

  **Work:** Start with failing Vitest/Playwright login and routing tests. Build `controller-web/src/` routes for Tasks, Workers, Settings, a compact application shell, same-origin typed fetch client, session bootstrap, CSRF handling, login/logout, expiry redirect, error boundary, and SSE reconnect/refetch signaling. Match existing Videnoa typography/color/icon conventions while keeping a restrained operational UI: no card grid, gradients, hero, or oversized controls. Do not retain the admin password, Bearer value, session, or CSRF proof in local/session storage; use the HttpOnly cookie and in-memory CSRF value.

  **Acceptance:** Unauthenticated users see login only; reload restores a valid session without credentials; logout/expiry clears state; keyboard focus/navigation work; embedded SPA routes resolve in release builds.

  **QA:** Happy—Playwright logs in, navigates, reloads, and logs out; save `.omo/evidence/videnoa-controller/task-15/shell-desktop.png` and `shell-narrow.png`. Failure—wrong password, expiry, network error, and malformed response show recoverable UI with no secret in storage/logs; save `.omo/evidence/videnoa-controller/task-15/shell-errors.txt`.

  **Commit:** `feat(controller-web): add authenticated application shell`

- [x] 16. Implement the dense server-paginated Tasks table and active counters

  **Depends on:** 7, 14, 15

  **References:** `.omo/knowledges/videnoa-controller-request.md:39-44`; original Tasks requirements in session `ses_fa2500002ffeEX1aBzRDiIy1q8`; `web/src/`.

  **Work:** Add failing component/browser tests, then build the central compact Tasks table with default columns Status, Name, Workflow, Worker, Progress, FPS, ETA, Size, Created, Finished, Source and optional path/attempt/duration/failure/error/remote-job columns. Bind URL query state to server-side filters, debounced search, sort/order, limit, and offset. Add compact All/Active/Queued/Processing/Failed/Finished counts. Apply SSE only to matching active rows; refetch the current page/counts when membership/order changes.

  **Acceptance:** Browser requests one bounded page, displays many rows per screen, never loads whole history, preserves query state in the URL, and stays usable with 20,000 seeded tasks.

  **QA:** Happy—filter/sort/search/page and apply an active-row delta without whole-table payload; save `.omo/evidence/videnoa-controller/task-16/tasks-table/`. Failure—invalid filters, empty page after live change, slow/failed calls, and long paths/errors stay bounded/readable; save `.omo/evidence/videnoa-controller/task-16/tasks-errors.txt`.

  **Commit:** `feat(controller-web): add dense task history table`

- [x] 17. Add manual task creation, detail pane, attempts, cancel, and stage-aware retry UX

  **Depends on:** 7, 9, 14-16

  **References:** `.omo/knowledges/videnoa-controller-request.md:12-17,39-44`; `.omo/drafts/videnoa-controller.md:56-61`; Todo 9.

  **Work:** Begin with failing form/detail/action tests. Add compact `+ Add Task` for input path, output path, workflow, priority, and source=`manual`; generate/reuse one idempotency key per submission. Do not add a media browser. Add a bottom detail pane with General, Progress, Attempts, and Error/Logs showing paths, timing, progress/FPS/ETA, remote job, attempt history, and failure data. Show cancel only through verifying and retry only when the API marks retryable; explain that path/collision changes require a new task.

  **Acceptance:** Manual creation equals automation intake; dropped-response replay does not duplicate; action availability matches server state; long values remain copyable without breaking density.

  **QA:** Happy—create `.mkv -> .mp4`, replay, inspect, cancel processing, and retry a processing failure; save `.omo/evidence/videnoa-controller/task-17/task-actions/`. Failure—outside roots, existing output, changed-body key reuse, ambiguity, and late cancel show exact guidance; save `.omo/evidence/videnoa-controller/task-17/task-action-errors.txt`.

  **Commit:** `feat(controller-web): add task creation and lifecycle controls`

- [x] 18. Implement Workers and Settings operational pages

  **Depends on:** 11, 14, 15

  **References:** `.omo/knowledges/videnoa-controller-request.md:26-32,39-44`; `.omo/drafts/videnoa-controller.md:50-52,62`.

  **Work:** Add failing page/action tests. Build a compact Workers table with name, URL, online/enabled state, used/total slots, processing/staged tasks, active upload/download, last seen/error, and add/edit/enable/disable actions with optimistic conflict recovery. Build Settings for pause/resume, upload/download limits, prefetch, timeout/retry values, and read-only data/temp/root/auth status. Root/data/auth-file changes remain restart-required configuration and secrets are never browser-editable. Explain that pause leaves processing and cleanup running.

  **Acceptance:** Operator manages worker capacity and runtime scheduler settings without DB edits; stale updates refetch; bounds are enforced; secrets are absent; SSE/API truth stays synchronized.

  **QA:** Happy—add workers, edit slots, disable, pause/restart/resume, and save `.omo/evidence/videnoa-controller/task-18/workers-settings/`. Failure—duplicate URL/name, stale version, busy delete, invalid limit, offline worker, and unauthenticated mutation show safe errors; save `.omo/evidence/videnoa-controller/task-18/workers-settings-errors.txt`.

  **Commit:** `feat(controller-web): add worker and scheduler management`

- [ ] 19. Complete frontend accessibility, responsive behavior, and browser regression coverage

  **Depends on:** 15-18

  **References:** `controller-web/` from Todos 15-18; `.omo/knowledges/videnoa-controller-request.md:39-44`.

  **Work:** Add Playwright configuration and stable test selectors only where semantic selectors are insufficient. Cover auth, create/list/filter/sort/page/details, workers, pause, cancel, retry, SSE reconnect, and errors. Add accessibility checks, keyboard navigation, dialog/detail focus restoration, visible focus, reduced motion, high-contrast statuses, responsive overflow/sticky controls, and CJK/long-path clipping checks. Keep pages bounded instead of adding client virtualization.

  **Acceptance:** Vitest/Playwright pass at desktop/narrow widths; no serious accessibility violations; core controls are keyboard-operable; table stays dense/readable; no credential material appears in storage or traces.

  **QA:** Happy—save full report/screenshots to `.omo/evidence/videnoa-controller/task-19/playwright-report/`. Failure—inject expiry, SSE disconnect, API 500, long CJK names, reduced motion, and narrow width; save `.omo/evidence/videnoa-controller/task-19/visual-failures/`.

  **Commit:** `test(controller-web): cover operational ui end to end`

### Wave 4 — Delivery and system proof

- [ ] 20. Prove one-worker and multi-worker pipelines under crash and outage injection

  **Depends on:** 6-14

  **References:** `.omo/knowledges/videnoa-controller-request.md:19-37`; `.omo/knowledges/videnoa-controller-repo-findings.md:34-39`; this plan’s required fault matrix.

  **Work:** Write failing integration scenarios before fixing uncovered behavior. Drive real Controller HTTP API plus one/three test-only mock Videnoa instances through create, upload, keyed run, long poll, download, verify, no-clobber publish, temp cleanup, remote cleanup, and retained history. Execute every Controller crash boundary, Videnoa restart-cancelled job, lost submit response, worker outage, partial transfer, cleanup 404/5xx, pause, retry, and cancellation state. Assert remote request counts and durable DB rows after each restart.

  **Acceptance:** Every scenario converges exactly as contracted; one remote job per attempt key; no duplicate task/output or stage regression; limits hold; successful work leaves no transient files but retains history.

  **QA:** Happy—save timelines, DB dump, request journals, and hashes under `.omo/evidence/videnoa-controller/task-20/happy/`. Failure—save fault assertions under `.omo/evidence/videnoa-controller/task-20/fault-matrix/`; fail on AI replay, overwrite, partial publication, leaked workspace, or slot overrun.

  **Commit:** `test(controller): prove crash-safe orchestration pipeline`

- [ ] 21. Add adversarial history-load, concurrency, filesystem, and security suites

  **Depends on:** 3, 4, 7, 11-14, 19

  **References:** `.omo/knowledges/videnoa-controller-request.md:34-49`; `crates/core/src/server/tests/files/security.rs:3-89`; this plan’s Verification strategy.

  **Work:** Seed 20,000+ tasks/attempts; race duplicate intake/reservation/publication; saturate independent pools; force EXDEV; swap symlinks; create destination files during publication; rotate auth hash; hammer login limits; lag/reconnect SSE; scan logs/artifacts/browser state for secrets. Record query plans/response sizes and run race-sensitive tests repeatedly against delayed/reordered responses.

  **Acceptance:** Supported queries use intended indexes; races preserve uniqueness/no-clobber; path/auth attacks fail closed; secrets are absent; pools remain independent; repeated runs are deterministic.

  **QA:** Happy—save load/query/concurrency results to `.omo/evidence/videnoa-controller/task-21/load-concurrency.txt`. Failure—save attack corpus, denied responses, secret scans, and filesystem races to `.omo/evidence/videnoa-controller/task-21/security-filesystem.txt`.

  **Commit:** `test(controller): add load and security regressions`

- [ ] 22. Add the dedicated GPU-free Controller container image

  **Depends on:** 1-21

  **References:** `Dockerfile:1-93`; `.github/workflows/unittest.yaml:180-225`; `.omo/drafts/videnoa-controller.md:58-59`.

  **Work:** Add failing container smoke/content checks, then create `Dockerfile.controller` with isolated dependency/frontend caching, GPU-free runtime base, non-root user, embedded assets, Controller entrypoint, data/config/temp volumes, and healthcheck. Document mounted admin hash and NAS roots; never bake secrets. Do not change existing GPU image behavior.

  **Acceptance:** Image starts without GPU flags/ORT/TensorRT/models, runs non-root, persists Controller data, and leaves current GPU Docker build intact.

  **QA:** Happy—build/run and save image history/file list/health to `.omo/evidence/videnoa-controller/task-22/container-happy.txt`. Failure—missing hash/config, unwritable data, and outside-root task fail clearly without leaks; save `.omo/evidence/videnoa-controller/task-22/container-errors.txt`.

  **Commit:** `build(controller): add gpu-free container image`

- [ ] 23. Add independent Linux and Windows Controller archive packaging

  **Depends on:** 1-21

  **References:** `scripts/package_dist.sh:70-96,207-251,410-465`; `scripts/package_dist.ps1:96-129,464-520`; `.omo/knowledges/videnoa-controller-repo-findings.md:27-32`.

  **Work:** Add failing archive-layout tests, then dedicated `scripts/package_controller.sh` and `.ps1`. Produce `videnoa-controller-v<version>-linux-x86_64.tar.gz` and `videnoa-controller-v<version>-windows-x86_64.zip`; each root contains only the binary, `controller.example.toml`, `README-controller.md`, and `LICENSE`. Assets are embedded. Version-gate, smoke where host-compatible, and reject GPU/model content. Do not alter existing package layouts.

  **Acceptance:** Scripts are deterministic, reject missing/forbidden files and version mismatch, and existing package smoke tests still pass.

  **QA:** Happy—save extracted manifests/checksums to `.omo/evidence/videnoa-controller/task-23/archive-manifests.txt`. Failure—missing required file, wrong version, or injected GPU library fails in `.omo/evidence/videnoa-controller/task-23/archive-errors.txt`.

  **Commit:** `build(controller): package standalone release archives`

- [ ] 24. Integrate Controller into CI and existing release conventions

  **Depends on:** 20-23

  **References:** `.github/workflows/unittest.yaml:19-225`; `.github/workflows/release.yaml:22-147,148-363,380-491`; `.omo/knowledges/videnoa-controller-repo-findings.md:27-32`.

  **Work:** Add CI jobs for Controller fmt/clippy/tests, web lint/test/build/e2e, fault/load suites, Linux/Windows archive smoke, and Controller image/content smoke. Extend release version gating and publish independent archives plus `controlnet/videnoa-controller:<workspace-version>` and `latest` with existing Docker Hub credentials. Preserve every existing Videnoa job/tag/asset and add assertions for complete non-overlapping release outputs.

  **Acceptance:** PR CI proves Controller and current products; release dry-run enumerates all assets; missing Controller artifacts or existing-product regressions cannot report success; no new registry/credential scheme exists.

  **QA:** Happy—save workflow/asset matrix to `.omo/evidence/videnoa-controller/task-24/ci-release-matrix.txt`. Failure—simulate missing asset/tag, version mismatch, current package break, and forbidden GPU content; save `.omo/evidence/videnoa-controller/task-24/ci-release-errors.txt`.

  **Commit:** `ci(controller): publish independent service artifacts`

- [ ] 25. Write Controller operator, API, security, recovery, and release documentation

  **Depends on:** 2-24

  **References:** `README.md`; `.omo/knowledges/videnoa-controller-request.md`; `.omo/knowledges/videnoa-controller-repo-findings.md`; all locked contracts in this plan.

  **Work:** Add `README-controller.md`, `docs/controller.md`, and `controller.example.toml`; add a concise root README section without renaming Videnoa. Document architecture, exact intake modes, hash generation/rotation, Bearer/session/CSRF, roots, Tailscale/HTTPS, persistent Controller and worker data, registration/workflows, scheduling/pause, lifecycle/retry/cancel, no-clobber/ambiguity response, backup/migration/rollback, Docker/archives, health/readiness, API examples, pagination, generic ANI-RSS calling, redaction, and troubleshooting. Use secret files/environment variables and no real/dummy production credential.

  **Acceptance:** A fresh operator can configure, launch, create tasks, diagnose/recover, back up/restore, upgrade/rollback, and understand non-goals without reading source.

  **QA:** Happy—execute all copy-pastable commands and save `.omo/evidence/videnoa-controller/task-25/docs-smoke.txt`. Failure—docs tests reject missing fields, plaintext secrets, stale endpoint/image/archive names, and broken links in `.omo/evidence/videnoa-controller/task-25/docs-errors.txt`.

  **Commit:** `docs(controller): add deployment and operations guide`

## Final verification wave

- [ ] F1. Plan compliance and scope-fidelity audit

  Read this plan, approved draft, and both knowledge files; inspect the complete diff and evidence. Verify every responsibility/acceptance criterion and every Must-NOT-Have, the existing product remains `videnoa`, and Controller is GPU/core-free. Reject missing stages, intake modes, GUI capabilities, artifacts/docs, or unrelated scope. Save `.omo/evidence/videnoa-controller/final/F1-plan-compliance.md`.

- [ ] F2. Code quality, security, and data-integrity review

  Run formatting, clippy, Rust/frontend tests, dependency/content scans, migrations, secret/log scans, and path/auth attacks; inspect exhaustive transitions, typed errors, module size, indexes/CAS, and no-clobber primitives. Reject panic request paths, permissive CORS, plaintext secrets, unsafe paths, overwrite fallback, blind resubmit, or misleading success logs. Save `.omo/evidence/videnoa-controller/final/F2-quality-security.md`.

- [ ] F3. Real manual QA and visual verification

  Launch a production-like Controller with a test-only password hash, NAS roots, and multiple mock Videnoa instances. Use Playwright for login, workers/settings, manual/API tasks, live pipeline, 20,000-row filters, pause, cancel/retry, Controller/worker restarts, output, and cleanup. Capture desktop/narrow/CJK screenshots plus DB/filesystem/remote evidence. Reject a card-heavy/sparse/inaccessible UI, clipping, credential leakage, or divergence from API/DB truth. Save `.omo/evidence/videnoa-controller/final/F3-manual-visual.md`.

- [ ] F4. Packaging, release, regression, and repository-landing audit

  Build/smoke the dedicated image and Linux/Windows archives; inspect linkage/content; run existing Videnoa workspace/web/package/GPU Docker/version checks; verify exact new names and unchanged old layouts. Test migration plus backup/rollback instructions. Reject GPU dependencies, existing regressions, incomplete release success, uncommitted/unpushed work, or branch not up-to-date. Save `.omo/evidence/videnoa-controller/final/F4-release-regression.md`.

## Commit strategy

- Each todo below includes its atomic commit message. Tests and the implementation they specify land together; do not batch unrelated todos into one commit.
- Before each commit: run the todo-local red/green commands, then `cargo fmt --all -- --check` or the corresponding frontend formatter/linter. Record evidence first and scan staged content for secrets.
- After each wave: run its aggregate Rust/frontend gates and inspect `git status --short --branch`; preserve unrelated user changes.
- Final landing sequence after every final verifier approves:

```bash
git pull --rebase
git push
git status --short --branch
```

Expected: push succeeds and status reports the branch up to date with its origin. Warn and resolve rather than force-pushing if rebase conflicts occur.

## Success criteria

- A NAS operator can configure canonical input/output roots, temp/data paths, one admin password hash, and one or more HTTP(S) Videnoa instances, then run Controller without GPU libraries.
- The Web UI and external authenticated API create the same durable task shape; duplicate HTTP delivery creates one task, and thousands of retained records remain paginated/filterable/sortable through indexed SQL.
- Controller schedules only compatible enabled workers, respects compute/prefetch/upload/download limits, feeds idle workers first, and persists scheduler pause across restart without killing active compute.
- Every remote compute attempt is durably idempotent. Controller and Videnoa crashes around submission produce at most one remote job per attempt; ambiguous evidence never causes automatic replay.
- Upload, processing, download, verification, publication, and cleanup resume from the correct durable stage. Downstream failures do not rerun successful AI processing.
- Different input/output extensions are preserved. Input is never modified. Partial output never appears at the final path, pre-existing output is never overwritten, and publication recovery either proves the expected artifact or stops as `publication_ambiguous`.
- Local temp files and remote task workspaces are removed before `completed`; cleanup retries survive restart; permanent Controller task and attempt history remains queryable.
- Login/session/Bearer/CSRF/rate-limit/path-boundary tests pass and captured logs/browser storage contain no admin secret or authentication material.
- Dense Tasks, Workers, and Settings pages pass Vitest, Playwright, accessibility, responsive, and active-row update checks without loading whole history.
- Linux/Windows Controller archives and `controlnet/videnoa-controller:<version|latest>` publish independently, contain no GPU runtime, and do not regress existing `videnoa` images, archives, builds, or tests.
