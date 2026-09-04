# F1 Plan Compliance and Scope-Fidelity Audit

Audit date: 2026-09-05

Audited implementation tip: `67620f5b45d54205b44861e3cc5d59e88724e998`

Comparison baseline only: `094d87833ebbc55c8f27857cd70d85fc5cc91afe`

Hosted authority: [GitHub Actions run 33910972161](https://github.com/ControlNet/videnoa/actions/runs/33910972161)

## Decision

**APPROVE**

The exact pushed tip satisfies all 25 implementation tasks, all ten plan Success Criteria, and every explicit Must-NOT-Have boundary. The Controller is an additive, independently runnable, GPU-free NAS product with durable SQLite orchestration, authenticated API and Web UI, crash-safe remote execution, no-clobber publication, cleanup, retained history, independent delivery artifacts, and operator documentation. Existing `videnoa`, `videnoa-desktop`, legacy Web, GPU image, archive layouts, release names, and regression jobs remain separate and passing.

This is a fresh audit of the plan, approved draft, current source and tests, task evidence, knowledge/notepads, implementation history, package/release contracts, and exact-tip hosted run. No approval or rejection was transferred from older final reports. The prior F1 rejection at `094d878` is superseded: the table overflow implementation now preserves explicit pre-growth `End` intent with `none`, `pending`, and `anchored` states, and the formerly contradictory browser gate passed 60/60 focused executions plus the complete 47/47 suite without retries, timeout changes, or weakened assertions.

No F1 blocker remains.

## Exact-Tip Authority and Required Gates

At audit start:

- `HEAD == origin/dev == 67620f5b45d54205b44861e3cc5d59e88724e998`.
- Branch divergence was `0 0` and the implementation worktree was clean.
- `git diff --check 094d87833ebbc55c8f27857cd70d85fc5cc91afe..67620f5b45d54205b44861e3cc5d59e88724e998` passed.
- A concurrent update to `F4-release-regression.md` appeared during this audit. It was not modified by F1 and is not used as sole proof for this verdict.

Run `33910972161` is exact-tip authority: it is a completed successful `push` run on `dev` whose `headSha` is exactly `67620f5b45d54205b44861e3cc5d59e88724e998`. All 14 jobs passed:

| Hosted job | Result |
|---|---|
| Workflow contracts | PASS |
| Rust tests (Ubuntu) | PASS |
| Rust tests (Windows) | PASS |
| Web build check (Ubuntu) | PASS |
| Web build check (Windows) | PASS |
| Controller Rust quality and tests | PASS |
| Controller fault and load suites | PASS |
| Controller Web quality and E2E | PASS |
| Controller archive smoke (Linux) | PASS |
| Controller archive smoke (Windows) | PASS |
| Controller image and content smoke | PASS |
| Package smoke (Linux) | PASS |
| Package smoke (Windows) | PASS |
| Docker build smoke | PASS |

Fresh independent exact-tip verification also passed:

- Controller Web: `npm ci --no-fund`, ESLint, TypeScript, production build, and two consecutive Vitest runs of `112/112`.
- Browser: focused overflow stress `60/60`; complete Chromium suite `47/47`, including authentication recovery, overflow, 20,000-row history, task actions, accessibility, responsive layouts, CJK content, forced colors, reduced motion, and empty browser storage.
- Controller Rust 1.83: formatting, strict Clippy, and `cargo test -p videnoa-controller --all-targets` all passed.
- Task 20: `30/30` crash, outage, cancellation, ownership, pause, retry, multi-worker, and shutdown cases passed.
- Task 21: `7 + 19 + 22 + 1 + 47 = 96/96` load, concurrency, filesystem, resource, and security cases passed.
- Existing products: repository-runtime `cargo test --workspace` passed, including `videnoa-core`, application, desktop, Controller, idempotency, and documentation targets.
- Delivery contracts: CI/release workflow positive and mutation-negative checks, deterministic Linux Controller archive contracts, and Controller documentation contracts passed locally; native Windows/archive/image authority is supplied by the exact-tip hosted jobs.
- Dependency isolation: `cargo tree -p videnoa-controller -i videnoa-core` reported no matching package in the Controller tree, and the forward Controller tree contained no `videnoa-core`, ORT, ONNX Runtime, CUDA, cuDNN, TensorRT, or NVIDIA dependency.

An intentionally over-broad, non-plan command using Rust 1.83 for the entire legacy workspace failed while parsing the legacy `ort-sys` Edition 2024 manifest. This is not a required gate: the plan pins Rust 1.83 to the isolated Controller package and uses the repository stable toolchain/runtime for existing Videnoa workspace regressions. The required Controller 1.83 gates and stable full-workspace gate both pass, so the result is reconciled and non-blocking.

## Tasks 1-25 Compliance Map

| Task | Result | Exact-tip audit basis |
|---:|---|---|
| 1 | PASS | `crates/controller` is a separate workspace package and binary with independent `controller-web` debug/release assets; package and HTTP contracts prove embedded SPA, disk assets, liveness, names, and GPU-free dependency isolation. |
| 2 | PASS | Branded IDs, stable snake_case DTOs/enums, exact path/extension values, strict layered configuration, locked defaults, URL bounds, and typed invalid-config failures pass domain/config/value/Windows contracts. |
| 3 | PASS | Six SQLx migrations, SQLite WAL/FK/busy settings, bounded pool, repositories, CAS transitions, atomic reservations, stable pages, and indexed 20,000-row plans pass fresh, upgrade, rollback, concurrency, and query tests. |
| 4 | PASS | One-admin Argon2id authentication, digest-only sessions, session/Bearer rotation, shared peer-IP password limiter, CSRF/same-origin mutation checks, redaction, and capability-rooted path race defenses pass hostile tests. |
| 5 | PASS | Additive Videnoa `/api/run` idempotency preserves unkeyed callers and converges sequential, concurrent, lost-response, restart, removed-workflow, and changed-content replay to one durable job per key. |
| 6 | PASS | The real-TCP mock Videnoa, request journals, checkpoints, stalls, disconnects, corruption, and cleanup faults remain under test support; production contains no dummy worker or fake-success route. |
| 7 | PASS | Authenticated `POST /api/tasks` enforces canonical idempotency and rooted immutable paths; list/detail/history APIs are bounded, indexed, searchable, filterable, sortable, and race-safe. |
| 8 | PASS | Typed health, catalog, compatibility, job, upload/download/stat/delete client operations enforce bounded JSON/streams, opaque remote paths, status taxonomy, timeouts, TLS URL parsing, and cache invalidation. |
| 9 | PASS | Exhaustive lifecycle, cancellation, retry, recovery, persist-before-side-effect, attempt replacement, downstream resume, and ambiguity policies pass typed transition and repository tests. |
| 10 | PASS | Startup recovery and coordinated shutdown dispatch every nonterminal stage, retain assignments through outages, drain bounded work, preserve ambiguous evidence, and never blindly resubmit unknown compute. |
| 11 | PASS | Worker CRUD/health, compatibility filtering, exact task/worker ordering, atomic capacity, idle-feed precedence, bounded prefetch, persisted pause, and independent upload/download limits pass. |
| 12 | PASS | Capability-rooted streamed transfer, exact input stat/hash/length checks, restart-from-zero downloads, partial cleanup, independent pools, and symlink/FIFO/replacement defenses pass. |
| 13 | PASS | Hidden task-owned destination staging, persisted expected hash/length/name, platform no-replace finalization, ambiguity preservation, local cleanup, remote cleanup, and `404` convergence pass. |
| 14 | PASS | Authenticated worker/settings/control/readiness/count/SSE operations, optimistic versions, cancel/retry policies, bounded deltas, passive auth, lag/refetch, and typed failures pass 28 direct tests and split security coverage. |
| 15 | PASS | The same-origin authenticated shell provides login, bootstrap, protected navigation, logout retry, in-memory CSRF, session expiry, embedded SPA fallback, recoverable focus, and no browser credential storage. |
| 16 | PASS | Dense server-paginated Tasks history handles 20,000 records, URL filters/search/sorts/pages, bounded active-row updates, local table overflow, sticky context, loading/error/empty states, and exact current-page correction. |
| 17 | PASS | Manual creation uses the same durable `POST /api/tasks` contract as external intake; detail/history, progress/errors, safe cancellation, and explicit processing retry guidance pass schema, policy, unit, and browser tests. |
| 18 | PASS | Dense Workers and Settings surfaces expose capacity, health, actions, pause, limits, retries/timeouts, optimistic recovery, and read-only roots/auth state without exposing editable secrets. |
| 19 | PASS | WCAG axe checks, keyboard overflow, dialog containment/focus restoration, SSE states, API recovery, long CJK content, forced colors, reduced motion, expiry, and empty browser storage pass; overflow stress is stable at 60/60. |
| 20 | PASS | Real Controller HTTP with one and three workers proves happy pipelines and every required crash/outage/pause/cancel/retry/cleanup/submission-ownership boundary with exact request counts and no duplicate compute. |
| 21 | PASS | Indexed 20,000-row load, snapshot consistency, duplicate intake/reservation/publication races, filesystem attacks, auth/CORS/SSE attacks, resource isolation, and secret/storage boundaries pass 96 split cases. |
| 22 | PASS | The dedicated Debian image builds Controller with Rust 1.83, embeds the SPA, runs as UID/GID 10001, persists required mounts, passes health/error/root smokes, and rejects legacy/GPU/model/runtime content. |
| 23 | PASS | Independently named Linux and Windows archives contain only the Controller binary, example config, Controller README, and license; deterministic/version/content/linkage gates pass without changing legacy layouts. |
| 24 | PASS | Exact-tip CI proves Controller and existing products on Linux/Windows. Release contracts preserve legacy assets/tags, require both Controller archives and image tags, share the existing credentials, and cannot report incomplete publication success. |
| 25 | PASS | Root README, archive first-run guide, operations/API/security/recovery guide, and example config cover setup, auth, roots, workers, scheduling, lifecycle, backup/restore, upgrade/rollback, deployment, releases, non-goals, and troubleshooting. |

## Success Criteria

| Criterion | Result | Evidence |
|---|---|---|
| GPU-free NAS configuration and worker onboarding | PASS | Controller builds/runs independently without GPU libraries; config, health, worker registration, container, archive, and dependency contracts pass. |
| Equivalent durable Web/API intake and scalable history | PASS | Both intake modes use authenticated idempotent `POST /api/tasks`; SQLite and browser tests prove bounded indexed retained history. |
| Compatible bounded scheduling and durable pause | PASS | Atomic reservation rechecks enabled/online/workflow/capacity/pause state; ordering, idle feed, prefetch, and transfer limits pass. |
| At most one remote job per compute attempt | PASS | Durable submission keys and ownership precede requests; replay converges to one job and ambiguity never authorizes automatic replay. |
| Correct durable-stage recovery without downstream compute replay | PASS | Task 20 covers all specified Controller/Videnoa crash and outage boundaries, retries, cancellation, and cleanup convergence. |
| Exact immutable paths and no partial/overwrite publication | PASS | Root capabilities, exact extensions, temp verification, hidden staging, no-replace finalization, and ambiguity fail-closed tests pass. |
| Cleanup before completion and permanent queryable history | PASS | Local and remote task workspaces converge before `completed`; task/attempt history remains retained, bounded, and indexed. |
| Authentication, path, CORS, SSE, and secret boundaries | PASS | Shared login/Bearer limiter, sessions, CSRF, path attacks, absent permissive CORS, authenticated SSE, logs, and browser storage pass. |
| Dense accessible operational UI | PASS | Two 112/112 unit runs, 60/60 overflow stress, and 47/47 full browser scenarios pass across responsive/accessibility states. |
| Independent delivery without existing-product regression | PASS | Exact-tip Linux/Windows Controller archive and image smokes plus legacy Rust/Web/archive/GPU-image jobs all pass; release contracts preserve names/layouts. |

## Scope and Must-NOT-Have Audit

| Constraint | Result | Audit basis |
|---|---|---|
| Preserve `videnoa`, `videnoa-desktop`, legacy Web, GPU image, archives, and release outputs | PASS | Existing workspace members, binaries, Dockerfile, package helpers, image tags, archive roots, and exact-tip Linux/Windows jobs remain separate and passing. |
| No Controller dependency or artifact path to core/GPU/model runtimes | PASS | Manifest, package tests, direct tree inspection, image inspection, archive linkage/content checks, and hosted smokes exclude `videnoa-core`, ORT, CUDA, cuDNN, TensorRT, NVIDIA, and models. |
| Exactly manual Web and generic external API intake | PASS | Both use authenticated `POST /api/tasks`; `source` remains informational and no ANI-RSS/qBittorrent semantics or adapter exists. |
| No watcher/discovery, rules, workflow deployment, media expansion, or Jellyfin refresh | PASS | No filesystem watcher, directory polling, cron discovery, rules engine, synchronization, media browser, or refresh subsystem exists in production. |
| No alternate transport/storage/control-plane stack | PASS | Remote work uses Videnoa HTTP `/api/files/*` and job APIs; authority is local SQLite, with no SSH/SFTP/rsync, object store, external database, broker, consensus, Kubernetes, mount manager, or GPU scheduler. |
| No multiple users, roles, accounts, or ACL | PASS | Authentication remains one administrator password with session and Bearer access only. |
| No permissive CORS, browser-to-worker calls, unbounded/card-heavy history, whole-table realtime, queue-wide pre-upload, or shared transfer semaphore | PASS | Same-origin API, absent CORS layer, shared API client, dense bounded tables, active deltas/refetch, bounded prefetch, and independent pools are implemented and tested. |
| No path/extension mutation, final-path download, overwrite fallback, or ambiguous-file deletion | PASS | Exact paths/extensions persist; input is reopened read-only; downloads stay in Controller temp; no-replace publication and ambiguity preservation fail closed. |
| No blind compute replay or automatic history deletion | PASS | Durable keys/ownership and typed remote/publication ambiguity prevent replay; downstream retries keep the attempt; completed task/attempt deletion is not implemented. |
| No unrelated implementation scope | PASS | The remediation range is limited to table navigation/focus, shared password throttling, exact dependency patches, matching tests, and audit documentation; release and legacy product shapes are unchanged. |

## Evidence Boundaries and Residual Risk

- Real GitHub Release creation and Docker Hub publication were not triggered by the `dev` push and are not claimed as executed. Task 24 explicitly accepts the release dry-run/contract graph; that graph requires all legacy and Controller assets/images before success and verifies both products after publication.
- GitHub retained no downloadable artifacts for run `33910972161`; approval relies on exact-tip hosted build/execute/verify logs, native Windows jobs, current source/tests, and independent local contract checks rather than claiming retained binaries.
- The complete workspace `cargo deny check` remains nonzero for separately scoped desktop/core advisories and existing license policy findings. Controller-reachable remediations are present (`anyhow 1.0.103`, `h2 0.4.16`, `rustls-webpki 0.103.13`), the prior advisory IDs are absent, and the isolated Controller dependency/content gates pass without policy ignores.
- Production `peer_ip` uses an `expect` for Axum `ConnectInfo`, which is installed by the only server construction path and covered by real-router/TCP tests. It is a framework invariant rather than unchecked external input; no request-data panic, overwrite fallback, or fake success path was found.
- Native Windows packaging and execution are proven by the exact-tip hosted Windows jobs; this Linux audit does not claim local native PowerShell execution.

## Verification Reviewed

- Read the complete plan, approved draft, request/repository knowledge, task-specific knowledge, notepads, current implementation history, source, tests, task evidence, container/archive scripts, workflows, documentation, and example configuration.
- Traced task intake through SQLite, wakeups, scheduler admission, upload, durable keyed submission ownership, polling, download, verification, no-clobber publication, local/remote cleanup, cancellation, retry, shutdown, and restart recovery.
- Independently mapped Tasks 1-13 and Tasks 14-25 against exact-tip source/tests; both reviews found every task implemented with no concrete blocker.
- Verified the former overflow rejection against the exact reported surface: 112/112 unit tests twice, 60/60 focused browser executions, and 47/47 complete browser scenarios all passed.
- Verified run `33910972161` directly: exact `headSha`, completed `success`, and all 14 required Controller and existing-product jobs successful.

VERDICT: APPROVE
