# F1 Plan Compliance and Scope-Fidelity Audit

Audit date: 2026-09-05

Audited implementation authority: `30d9f25d19cf0ec1a88733483da7f95581e980ad`

Implementation baseline: `5c099b1f71d10920bc054f0994452d0b937294b1` (parent of the first Controller implementation commit)

Hosted authority: [GitHub Actions run 33946244764](https://github.com/ControlNet/videnoa/actions/runs/33946244764)

## Decision

**APPROVE**

The exact pushed tip satisfies all Tasks 1-25, all plan acceptance criteria, the complete Success Criteria, and every scope exclusion/Must-NOT-Have. The prior blockers remain closed: the legacy Linux package smoke creates and verifies the split archive under bounded p7zip resources; missing peer metadata returns a typed internal response instead of panicking; startup and recurring recovery use finite high-water keyset pagination; and the final authentication executor-starvation defect is remediated without expanding product scope.

Run `33946244764` is a completed successful `push` run for this exact SHA, and all 14 required jobs passed. This `dev` push did not create a GitHub Release or publish Docker Hub images; approval covers source, contracts, packaging smoke, and release wiring, not an unexecuted production publication.

This is an F1-only decision. The stale F4 rejection at `e161e772744c48791f67cb21575c6ebef4ace13c` and run `33940048214` describes the pre-remediation failure and reviewer-output landing state; it is not inherited as an F1 implementation blocker and is not treated as F4 approval. Initial language acceptance is English and Chinese only. Korean remains explicitly out of scope and is neither claimed supported nor fixed.

## Exact-Tip and Range Authority

- `GIT_MASTER=1 git status --short --branch` reported `dev...origin/dev` plus eight modified reviewer artifacts, all under `.omo/`: F1/F2/F3/F4 reports, F3 knowledge, issues/learnings notepads, and the Controller plan. `git diff --name-only -- . ':(exclude).omo/**'` returned no paths, so implementation/source was clean at audit start.
- `GIT_MASTER=1 git rev-parse HEAD origin/dev` returned the audited SHA twice.
- `GIT_MASTER=1 git rev-list --left-right --count HEAD...origin/dev` returned `0 0`.
- `GIT_MASTER=1 git ls-remote origin refs/heads/dev` independently returned the audited SHA.
- The complete implementation range contains 239 commits and 450 changed paths (`66,951` insertions, `125` deletions). The full name/status, stat, chronological commit list, and protected-product path diff were inspected.
- Existing `crates/app/`, `crates/desktop/`, `web/`, `scripts/package_dist.sh`, and `scripts/package_dist.ps1` are unchanged across the implementation range. Root `Dockerfile` changes only enumerate the new workspace manifest/dummy cache source, as required by Task 1; its final binary, command, GPU runtime, and image identity remain `videnoa`.
- `crates/core` changes are limited to the additive, backward-compatible `/api/run` idempotency contract plus tests. Existing unkeyed requests remain supported.
- The final tip's parent is `e161e772744c48791f67cb21575c6ebef4ace13c`. Commit `30d9f25` changes only nine Controller authentication/error/test paths; it does not touch legacy Videnoa, core, frontend, packaging, Docker, workflow, or release files.

## Hosted CI Authority

GitHub CLI and the read-only Actions API confirm run `33946244764` is `completed/success`, event `push`, branch `dev`, head SHA exactly `30d9f25d19cf0ec1a88733483da7f95581e980ad`, attempt `1`, workflow `.github/workflows/unittest.yaml`. The jobs API reports `total_count: 14`, and all 14 jobs are `completed/success`.

| Required job | Job ID | Result |
|---|---:|---|
| Web build check (ubuntu-latest) | `101252744428` | PASS |
| Rust tests (ubuntu-latest) | `101252744449` | PASS |
| Controller web quality and E2E | `101252744475` | PASS |
| Workflow contracts | `101252744485` | PASS |
| Web build check (windows-latest) | `101252744489` | PASS |
| Rust tests (windows-latest) | `101252744529` | PASS |
| Controller Rust quality and tests | `101252744542` | PASS |
| Package smoke (Linux) | `101253220716` | PASS |
| Docker build smoke | `101253220736` | PASS |
| Package smoke (Windows) | `101253220760` | PASS |
| Controller archive smoke (Linux) | `101253918580` | PASS |
| Controller image and content smoke | `101253918594` | PASS |
| Controller archive smoke (Windows) | `101253918618` | PASS |
| Controller fault and load suites | `101253918644` | PASS |

The remediated Linux job passed bundle construction, runtime compatibility, bounded split-archive creation, and archive layout/split-volume verification. The Controller fault/load job passed both crash/outage and load/concurrency/filesystem/resource/security steps, including the authentication-contention regression. This smoke workflow is not evidence of production publication.

## Final Authentication Executor-Starvation Remediation

- The pre-fix exact-tip run `33940048214` failed only `task_api::concurrency::concurrent_duplicate_intake_creates_exactly_one_task` with HTTP `500`; constrained reproduction identified SQLite `PoolTimedOut` during idempotency preflight while synchronous Argon2 Bearer verification starved Tokio executor progress.
- Commit `30d9f25` adds `PasswordFile::verify`, which moves password-file loading and Argon2 verification together into `tokio::task::spawn_blocking`. Login and Bearer authentication await that boundary while preserving success, unauthorized, rate-limit, fingerprint, session, and CSRF behavior.
- Bearer authentication becomes async through both active and passive middleware paths. The new `PasswordVerification` join-failure variant maps exhaustively to the existing typed internal-error/readiness behavior.
- The regression uses an explicit nine-party barrier for eight simultaneous authenticated intake requests, one SQLite connection, and a 100 ms busy timeout. It requires exactly one `201`, seven `200` replays, and one durable task.
- Fresh local execution of `cargo +1.83.0 test --locked -p videnoa-controller --test task_api concurrent_duplicate_intake_creates_exactly_one_task -- --nocapture` passed `1/1`; hosted run `33946244764` supplies the complete exact-SHA regression authority.
- Synchronous hash-file fingerprint reads remain in cookie-session rotation validation and readiness. They perform no Argon2 work and are outside the specific demonstrated executor-starvation defect; no broader authentication rewrite is claimed.

## Tasks 1-25 Compliance Map

| Task | Result | Responsibility and acceptance assessment |
|---:|---|---|
| 1 | PASS | `crates/controller/` is an independent workspace package/binary with isolated `controller-web/` release embedding and debug disk serving. Builds, health/SPA tests, missing-assets errors, Cargo metadata, and unchanged existing binary targets are evidenced by Task 1 and hosted Controller Rust/Web jobs. |
| 2 | PASS | Branded IDs, exhaustive snake_case lifecycle/failure enums, DTOs, exact path/extension preservation, strict config parsing, locked defaults, URL/root/timeout/slot/page bounds, and typed startup failures are implemented and covered by `task-2/contracts.json` and `config-errors.txt`. |
| 3 | PASS | SQLx SQLite migrations provide WAL, foreign keys, bounded pooling, required tables/columns/uniqueness/indexes, CAS transitions, atomic reservation/capacity, recovery queries, and stable indexed 20,000-row pages. Migration, rollback, contention, and query-plan suites pass. |
| 4 | PASS | One-admin Argon2id hash-file auth, digest-only sessions, expiry/rotation/logout, CSRF, same-origin cookies, shared API-authentication throttling, redaction, and capability-rooted no-follow NAS paths pass hostile auth/path tests. Missing peer metadata maps to typed `500 internal_error`, and password loading plus Argon2 verification for login/Bearer paths runs outside async executor workers. |
| 5 | PASS | Videnoa `/api/run` accepts optional durable `Idempotency-Key`: first keyed request creates, same key/body replays, changed body conflicts, concurrent duplicates create one job, restart preserves lookup, and unkeyed legacy clients remain compatible. |
| 6 | PASS | The mock Videnoa is confined to `crates/controller/tests/support/`, supplies deterministic health/catalog/file/job/restart/fault controls and request journals, and is excluded from production dependencies, binary, archives, and image checks. |
| 7 | PASS | Manual and external intake share authenticated `POST /api/tasks`; durable canonical idempotency, immutable rooted paths, input snapshots, explicit output nonexistence, bounded pagination, allowlisted filters/search/sorts, stable ties, and 20,000-row behavior pass. The final barrier-based authenticated duplicate-intake regression proves one create plus seven replays under constrained executor/SQLite contention. |
| 8 | PASS | The typed reqwest client covers health, workflow+preset discovery, exact `Path` input/output compatibility, keyed run/poll/cancel, bounded JSON, TLS/timeouts, streamed file APIs, opaque remote paths, cache invalidation, and typed status/network errors. |
| 9 | PASS | The central lifecycle exhaustively covers all 14 states, legal/illegal transitions, persist-before-side-effect commands, bounded transient retries, explicit processing retry, downstream same-attempt retry, cancellation limits, and non-retryable remote/publication ambiguity without history erasure or compute replay. |
| 10 | PASS | Startup reconciliation and graceful shutdown cover every nonterminal state and required crash boundary; remote outage/DB loss fail safely without reassignment or blind replay. Recovery now snapshots a finite `(updated_at_ms,id)` high-water and consumes strict keyset pages to completion. |
| 11 | PASS | Worker registry/versioning, URL/name uniqueness, health/capability refresh, atomic slots, deterministic task/worker order, per-worker upload/prefetch, independent global transfer pools, idle-feed precedence, disable/delete policy, and durable pause semantics pass concurrency/restart tests. |
| 12 | PASS | Upload/download stages persist before I/O, stream with bounded memory, recheck input identity and remote length, restart partial transfers from zero, preserve independent extensions/opaque paths, verify nonzero length/SHA-256, and use independent limits without rerunning compute. |
| 13 | PASS | Publication persists hash/length/staging evidence, uses destination-owned hidden staging and platform no-replace finalization, preserves racing/mismatching files, recovers finalization by hash, cleans local temp then remote workspace, treats remote 404 idempotently, and never repeats AI. |
| 14 | PASS | Authenticated worker/settings/control/readiness/count/SSE routes expose typed statuses, optimistic versions, legal cancel/retry, bounded active deltas/refetch, and no auth material. Browser traffic remains same-origin to Controller only. |
| 15 | PASS | The React shell implements login/session bootstrap/logout/expiry, in-memory CSRF, same-origin typed fetch, protected Tasks/Workers/Settings routes, error recovery, keyboard focus, embedded SPA routing, and empty browser storage for credentials. |
| 16 | PASS | Tasks is a dense server-paginated table with the required default/optional fields, URL-bound filter/search/sort/page state, compact counters, bounded requests, active-row merge/refetch semantics, long-value containment, and 20,000-row usability. |
| 17 | PASS | Compact manual creation generates/reuses one submission key, preserves `.mkv -> .mp4` choices, and shares backend intake. Detail/attempt/progress/error surfaces plus stage-aware cancel/retry controls and collision guidance pass component/E2E tests. |
| 18 | PASS | Workers and Settings provide compact CRUD/enable-disable/capacity/health, scheduler pause, concurrency/timeouts/retries, optimistic conflict refetch, bounds, and restart-required read-only path/auth state without exposing secrets. |
| 19 | PASS | Hosted lint/unit/build/Chromium E2E pass. Evidence covers desktop/narrow/CJK, axe, keyboard/focus restoration, reduced motion, forced colors, responsive table overflow, shell scrolling, and non-overlapping logout errors; no credential material appears in storage/traces. |
| 20 | PASS | Real Controller HTTP plus one/three mock workers cover create-upload-keyed run-poll-download-verify-publish-cleanup, all specified crash/outage boundaries, pause/cancel/retry, persistent rows, one request/job per attempt key, no duplicate output, no slot leak, and no transient leftovers. |
| 21 | PASS | Adversarial suites seed 20,000+ rows, verify indexes/payload bounds, race intake/reservation/publication, saturate independent pools, exercise symlink/root/auth/session/CORS/SSE/resource attacks, and scan for credential leakage. Hosted fault/load/security authority passes at the remediated exact SHA. |
| 22 | PASS | `Dockerfile.controller` builds isolated embedded assets and a Rust 1.83 binary, runs Debian bookworm-slim as `10001:10001`, declares health/persistent mounts, has no GPU flags/runtime/models/core/legacy binary, and passes startup, persistence, root, and content checks. |
| 23 | PASS | Linux and native Windows Controller archive jobs pass exact version/name/root/content contracts. Archives are `videnoa-controller-v0.1.2-linux-x86_64.tar.gz` and `videnoa-controller-v0.1.2-windows-x86_64.zip`, containing only binary, `controller.example.toml`, `README-controller.md`, and `LICENSE`; legacy Linux/Windows package smoke also passes. |
| 24 | PASS | CI contains all Controller Rust/Web/fault/load/archive/image jobs while retaining every existing product job. Release wiring version-gates both products, requires all packages/images before GitHub Release, preserves legacy tags/assets, and defines independent `controlnet/videnoa-controller:0.1.2` and `latest` publication. Positive and mutation-negative workflow contracts pass. |
| 25 | PASS | Root/archive/operator docs and example config cover architecture, exact intake modes, auth/rotation/CSRF, roots, HTTP(S)/Tailscale, worker persistence, scheduling/lifecycle, no-clobber/ambiguity, backup/migration/rollback, health/readiness, API pagination, generic ANI-RSS calling, archives/images, troubleshooting, and non-goals. Documentation contracts pass. |

## Success Criteria

| Criterion | Result | Exact-tip basis |
|---|---|---|
| GPU-free NAS configuration and worker onboarding | PASS | Isolated dependency graph, config/root validation, worker registry/capabilities, Controller archives, and image/content checks pass. |
| Equivalent durable Web/API intake and scalable history | PASS | Both callers use idempotent `POST /api/tasks`; SQLite-backed bounded/indexed API and dense UI behavior pass at 20,000+ rows. |
| Compatible bounded scheduling and durable pause | PASS | Workflow eligibility, deterministic capacity/order, prefetch, idle-feed priority, independent pools, and pause/restart contracts pass. |
| At most one remote job per compute attempt | PASS | Durable remote key plus submission ownership, replay/conflict/restart, and outage request-count tests pass. |
| Correct durable-stage recovery without downstream compute replay | PASS | Every lifecycle recovery action, crash/outage matrix, finite keyset scan, stage retry, cancellation, and ambiguity behavior pass. |
| Exact immutable paths and no partial/overwrite publication | PASS | Capability-rooted input/output, independent extensions, temp verification, hidden staging, no-replace finalization, and ambiguity preservation pass. |
| Cleanup before completion and permanent queryable history | PASS | Local and remote cleanup convergence precedes completion; retained task/attempt history remains bounded and queryable. |
| Authentication, path, CORS, SSE, and secret boundaries | PASS | Session/Bearer/CSRF/rate-limit/rotation, off-executor Argon2 verification, missing-peer typed failure, hostile paths, absent permissive CORS, bounded SSE, and redaction checks pass. |
| Dense accessible operational UI | PASS | Tasks/Workers/Settings, desktop/narrow/CJK, keyboard/focus, axe, forced colors, reduced motion, and active-row/browser tests pass. |
| Independent delivery without existing-product regression | PASS | Both Controller archives/image and both legacy package paths pass at the exact SHA; release outputs remain independent and non-overlapping. |

## Scope and Must-NOT-Have Audit

| Constraint | Result | Exact-tip assessment |
|---|---|---|
| Existing product identity and layout | PASS | Existing products remain `videnoa` and `videnoa-desktop`; legacy image tags remain `controlnet/videnoa:<version|latest>`, legacy archives remain `videnoa-linux64-<version>.7z*` and `videnoa-win64-<version>.7z*`, and the `videnoa/` archive root/layout is preserved. |
| Controller independence and GPU/core exclusion | PASS | Controller has no normal/build path to `videnoa-core`, ORT/ONNX Runtime, CUDA, cuDNN, TensorRT, NVIDIA runtime, models, or GPU scheduling. Root Dockerfile enumeration is cache-only and does not build or ship Controller in the GPU image. |
| Intake/discovery exclusions | PASS | Only manual Web and generic external API intake exist; `source` is metadata. No watcher, directory poller, ANI-RSS/qBittorrent adapter, cron discovery, rules engine, workflow deployment/sync, media browser expansion, or Jellyfin refresh integration exists. |
| Transport/storage/distributed exclusions | PASS | Controller uses credential-free HTTP(S) Videnoa APIs and SQLite. No SSH/SFTP/rsync, managed remote mount, S3/object storage, resumable protocol, Redis/PostgreSQL requirement, broker, consensus, Kubernetes, GPU-ID/VRAM/device scheduler, or generic event store exists. |
| Identity/auth/CORS exclusions | PASS | One administrator only; no user/role/account/ACL system, plaintext persisted/logged credential, browser-stored password/session/CSRF proof, permissive CORS, or direct browser-to-Videnoa calls were found. Missing peer metadata fails closed without fabricated forwarding-header trust. |
| History/realtime/queue exclusions | PASS | No unbounded listing, card-heavy history, whole-table realtime replacement, queue-wide pre-upload, or shared upload/download semaphore exists. History is dense, paginated, indexed, retained, and SSE is bounded/ephemeral. |
| Path/publication exclusions | PASS | No guessed/normalized output extension, `input_path` mutation/deletion, direct final-directory download, overwrite/auto-rename fallback, cross-device unsafe rename, or deletion of ambiguous/unrelated final files exists. |
| Replay/retry/history exclusions | PASS | No blind AI resubmission after timeout/crash/missing evidence/worker DB loss/downstream failure, and no automatic deletion of completed task/attempt history exists. Durable task and remote keys plus submission ownership enforce at most one remote job per attempt. |
| Scope discipline | PASS | The complete range implements the approved Controller, additive remote idempotency, independent delivery, tests/evidence, and required legacy-package reliability only; no unrelated product feature or rename was introduced. |

The language scope adds no Korean requirement. English and Chinese evidence is accepted for the initial release; the known Korean rendering observation remains out of scope and unfixed.

## Verification Evidence

- `cargo +1.83.0 test --locked -p videnoa-controller --test task_api concurrent_duplicate_intake_creates_exactly_one_task -- --nocapture`: PASS, `1/1` at `30d9f25`.
- `cargo +1.83.0 test --locked -p videnoa-controller --test task_api direct_router_returns_typed_internal_error_when_peer_metadata_is_missing -- --nocapture`: PASS, `1/1`.
- `cargo +1.83.0 test --locked -p videnoa-controller --test mock_videnoa startup_scans_durable_tasks_and_dispatches_recovery_commands -- --nocapture`: PASS, `1/1`; page size two covers all equal-timestamp nonterminal fixtures.
- `cargo +1.83.0 test --locked -p videnoa-controller --test task20 active_first_page_cannot_starve_later_durable_work -- --nocapture`: PASS, `1/1`; page size one does not let an older active task starve later durable work.
- `bash scripts/tests/package_dist_archive_test.sh`: PASS for split/single output, bounded resource command, missing output, insufficient space, and exact fatal-exit propagation.
- `bash scripts/tests/package_controller_test.sh`: PASS for deterministic Linux Controller name/version/layout and forbidden-content rejection.
- `node scripts/tests/validate_ci_release_workflows.test.mjs`: PASS for the complete positive matrix and every negative mutation, including existing-product break and Linux helper bypass.
- `bash scripts/tests/controller_docs_test.sh`: PASS.
- `cargo +1.83.0 tree --locked -p videnoa-controller --edges normal,build` at `30d9f25` plus a forbidden dependency inspection found no `videnoa-core`, ORT/ONNX Runtime, CUDA, cuDNN, or TensorRT path.
- `git diff --check 30d9f25^..30d9f25`: PASS.

## Evidence Boundaries

- Local/source verification establishes repository state, dependency isolation, exact contracts, focused blocker regressions, names/layouts, workflow wiring, and documentation.
- Hosted run `33946244764` establishes exact-SHA Linux/Windows existing-product tests/builds/packages, Controller Rust/Web/fault/load/archive/image checks, the remediated concurrent-intake path, and successful legacy Linux split-archive verification.
- This run is a `dev` push smoke run with no retained artifacts. It did not create a release tag or GitHub Release and did not push either `controlnet/videnoa` or `controlnet/videnoa-controller` tags to Docker Hub. Those publication steps remain correctly gated in `.github/workflows/release.yaml`.
- Dirty `.omo` reports, notepads, knowledge, and plan state are reviewer output. They do not make the exact committed product/source tree dirty and are not represented as committed or pushed evidence.
- The current F4 report's rejection predates `30d9f25` and run `33946244764`. F1 does not infer an F4 disposition from this audit.
- No secret, credential value, cookie value, authentication header value, password, or CSRF proof is included in this report.

VERDICT: APPROVE
