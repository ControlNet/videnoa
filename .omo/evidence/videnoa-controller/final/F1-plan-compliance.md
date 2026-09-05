# F1 Plan Compliance and Scope-Fidelity Audit

Audit date: 2026-09-05

Audited implementation tip: `d8830fa55cf54d504eafbc768747e57f3d2dcadf`

Remediation comparison baseline: `67620f5b45d54205b44861e3cc5d59e88724e998`

Hosted authority: [GitHub Actions run 33919997302](https://github.com/ControlNet/videnoa/actions/runs/33919997302)

## Decision

**REJECT**

The exact pushed tip does not satisfy the plan's complete CI and existing-product regression obligations. Run `33919997302` completed with conclusion `failure`: 13 of 14 jobs passed, but legacy `Package smoke (Linux)` failed while creating the required split archive. Its bundle build and Linux compatibility check passed, then p7zip 16.02 scanned 5,335,491,267 bytes, emitted `System ERROR: E_FAIL`, and exited `2`; archive-layout and split-volume verification was consequently skipped.

This is F1-blocking even though the dedicated Controller Linux and Windows archive jobs passed and no Controller product defect was demonstrated. Task 23 requires existing package smoke tests to remain passing; Task 24 requires CI to prove Controller and current products and forbids existing-product regressions from reporting success; the final Success Criteria require Controller delivery without regressing existing `videnoa` archives, builds, or tests. Exact-tip hosted authority is therefore red and cannot support approval.

The prior F3 shell findings were independently reassessed and are no longer blockers. Desktop Settings now scrolls through `.shell-main` while the document/frame remain fixed and Sign out remains visible; the narrow logout failure alert occupies its own grid row, keeps its focus outline within the viewport, creates no horizontal overflow, and overlaps no visible enabled control. The rejection is based on the exact-tip release/regression failure, not the remediated shell behavior.

## Exact-Tip Authority

At audit start:

- `HEAD == origin/dev == d8830fa55cf54d504eafbc768747e57f3d2dcadf`.
- Branch divergence was `0 0` and the worktree was clean.
- `git diff --check 67620f5b45d54205b44861e3cc5d59e88724e998..d8830fa55cf54d504eafbc768747e57f3d2dcadf` passed.
- The remediation range contains one Controller Web source/test commit followed by notepad, final-report, and plan-checkbox documentation commits; no release workflow, packaging script, Dockerfile, manifest, or lockfile changed.

Run `33919997302` is the required exact-tip authority: it is a completed push run on `dev` whose `headSha` is exactly `d8830fa55cf54d504eafbc768747e57f3d2dcadf`.

| Hosted job | Job ID | Result |
|---|---:|---|
| Workflow contracts | `101175899842` | PASS |
| Web build check (Ubuntu) | `101175899980` | PASS |
| Rust tests (Ubuntu) | `101175900036` | PASS |
| Controller Web quality and E2E | `101175900048` | PASS |
| Controller Rust quality and tests | `101175900066` | PASS |
| Web build check (Windows) | `101175900074` | PASS |
| Rust tests (Windows) | `101175900102` | PASS |
| Package smoke (Windows) | `101176805366` | PASS |
| Docker build smoke | `101176805442` | PASS |
| Package smoke (Linux) | `101176805555` | **FAIL** |
| Controller fault and load suites | `101178383467` | PASS |
| Controller archive smoke (Linux) | `101178383474` | PASS |
| Controller archive smoke (Windows) | `101178383529` | PASS |
| Controller image and content smoke | `101178383579` | PASS |

The failing Linux job successfully ran legacy archive contracts, built the complete package bundle, verified Linux runtime compatibility, and reclaimed caches. The failure occurred in `Create split archive (2000MB volumes)` before the required archive verification. Windows completed the equivalent build, split-archive creation, and verification successfully.

## Tasks 1-25 Compliance Map

| Task | Result | Exact-tip audit basis |
|---:|---|---|
| 1 | PASS | `videnoa-controller` remains an isolated workspace package and binary with independent embedded/debug Web assets and no core/GPU dependency. |
| 2 | PASS | Typed domain, lifecycle, configuration, DTO, path, URL, and bound contracts remain implemented and covered by passing Controller tests. |
| 3 | PASS | SQLite migrations, WAL/FK settings, repositories, CAS transitions, reservations, indexes, and bounded history queries pass hosted Controller and load suites. |
| 4 | PASS | Single-admin Argon2id auth, digest-only sessions, CSRF, throttling, redaction, and capability-rooted NAS paths pass security and fault/load authority. |
| 5 | PASS | Videnoa `/api/run` durable idempotency remains additive and covered by passing Ubuntu/Windows Rust and Controller suites. |
| 6 | PASS | Mock Videnoa and deterministic fault controls remain test-only; production dependency/content checks pass. |
| 7 | PASS | Task intake requires durable idempotency and rooted immutable paths; history remains bounded, indexed, filterable, searchable, and sortable. |
| 8 | PASS | Typed Videnoa health/catalog/job/file clients retain bounded streaming, timeouts, status mapping, opaque remote paths, and compatibility checks. |
| 9 | PASS | Exhaustive lifecycle, retry, cancellation, ambiguity, and persist-before-side-effect contracts pass Controller tests. |
| 10 | PASS | Startup reconciliation and graceful shutdown cover nonterminal stages without blind compute replay or capacity loss. |
| 11 | PASS | Worker registry, deterministic scheduling, capacity, prefetch, independent transfer pools, and durable pause pass hosted suites. |
| 12 | PASS | Restart-safe streamed upload/download, stat/hash/length checks, partial cleanup, and independent limits remain covered. |
| 13 | PASS | Hidden staging, hash-backed recovery, platform no-replace publication, and local/remote cleanup convergence pass. |
| 14 | PASS | Authenticated worker/settings/control/readiness/count/SSE APIs retain optimistic versioning, bounded deltas, and typed failures. |
| 15 | PASS | The authenticated same-origin shell retains session bootstrap, in-memory CSRF, protected routing, logout recovery, and empty browser storage. |
| 16 | PASS | Dense bounded Tasks history retains server pagination, URL query state, active-row updates, and table-owned overflow. |
| 17 | PASS | Manual intake, idempotent replay, task detail/attempts, cancel, and stage-aware retry remain implemented and tested. |
| 18 | PASS | Workers and Settings retain compact operations, capacity, pause, concurrency/timeouts/retries, stale-write recovery, and read-only secret/root state. |
| 19 | PASS | Exact-tip Web E2E passed; focused local tests and manual Chromium checks confirm desktop scroll/footer and narrow alert containment remediation. |
| 20 | PASS | Exact-tip crash and outage suite passed complete pipeline, restart, idempotency, pause, cancellation, transfer, publication, and cleanup coverage. |
| 21 | PASS | Exact-tip load, concurrency, filesystem, resource, and security suites passed, including indexed 20,000-row and hostile-boundary coverage. |
| 22 | PASS | Dedicated Controller image/content smoke passed with non-root, embedded, persistent, GPU-free product boundaries. |
| 23 | **FAIL** | Dedicated Controller archives passed, but this task also requires existing package smoke tests to pass; legacy Linux split-archive creation failed and verification was skipped. |
| 24 | **FAIL** | Exact-tip CI completed `failure`, so it does not prove all current products and does not satisfy the no-existing-regression acceptance criterion. |
| 25 | PASS | Controller operator, API, security, recovery, deployment, archive/image, and example-configuration documentation remains present and contract-tested. |

## Success Criteria

| Criterion | Result | Evidence |
|---|---|---|
| GPU-free NAS configuration and worker onboarding | PASS | Dependency, image, archive, configuration, health, and worker contracts passed. |
| Equivalent durable Web/API intake and scalable history | PASS | Idempotent intake plus indexed bounded SQLite/API/UI history passed. |
| Compatible bounded scheduling and durable pause | PASS | Scheduler, capacity, ordering, prefetch, pause, and independent transfer limits passed. |
| At most one remote job per compute attempt | PASS | Durable submission ownership/idempotency and ambiguity handling passed. |
| Durable-stage recovery without downstream compute replay | PASS | Exact-tip crash/outage and recovery suites passed. |
| Exact immutable paths and no partial/overwrite publication | PASS | Capability paths, temp verification, hidden staging, no-replace, and ambiguity tests passed. |
| Cleanup before completion and permanent queryable history | PASS | Cleanup convergence and retained indexed history passed. |
| Authentication, path, CORS, SSE, and secret boundaries | PASS | Exact-tip security suites and browser-storage checks passed. |
| Dense accessible operational UI | PASS | Exact-tip Web quality/E2E passed; fresh focused shell tests passed `2/2` and manual desktop/narrow Chromium checks passed. |
| Independent delivery without existing-product regression | **FAIL** | Controller archives/images passed, but legacy Linux package split-archive creation failed on the exact tip. |

## Scope and Must-NOT-Have Audit

| Constraint | Result | Audit basis |
|---|---|---|
| Preserve existing `videnoa`, desktop, Web, GPU image, archive names/layouts, and releases | **FAIL at verification gate** | Product identities and layouts remain unchanged, but the exact-tip legacy Linux archive smoke did not complete successfully. |
| No Controller dependency/artifact path to core, ONNX Runtime, CUDA, cuDNN, TensorRT, NVIDIA, or models | PASS | Manifest/tree/content/image/archive checks remain clean; the inverse tree query finds no Controller/core relation. |
| Only manual Web and generic external API intake; `source` informational | PASS | Both intake modes use authenticated `POST /api/tasks`; no ANI-RSS/qBittorrent adapter exists. |
| No watchers, discovery, rules, workflow deployment, media expansion, or Jellyfin refresh | PASS | No prohibited production subsystem was found. |
| No alternate transport, external authority store, broker, consensus, Kubernetes, mount, or GPU scheduler | PASS | Controller uses SQLite plus Videnoa HTTP/file APIs only. |
| No multiple users, roles, accounts, ACL, plaintext/browser-stored credentials, or permissive CORS | PASS | One-admin session/Bearer model, redaction, empty storage, CSRF, and absent permissive CORS remain enforced. |
| No unbounded/card-heavy history, whole-table realtime, queue-wide pre-upload, or shared transfer semaphore | PASS | Bounded dense tables, active deltas/refetch, bounded prefetch, and independent upload/download pools remain implemented. |
| No path/extension mutation, input deletion, final-path download, overwrite fallback, or ambiguous-file deletion | PASS | Immutable paths, Controller temp, no-replace publication, and fail-closed ambiguity remain covered. |
| No blind compute replay or automatic completed-history deletion | PASS | Durable keys/ownership and ambiguity rules block replay; history deletion is not implemented. |
| No unrelated implementation scope | PASS | Exact-tip remediation is limited to shell CSS/tests and audit documentation; release/package implementation was not changed. |

## Fresh Shell Verification

- `npx playwright test tests/e2e/shell.spec.ts --grep "desktop Settings wheel scroll|logout failure keeps" --reporter=line --output=/tmp/opencode/videnoa-f1-playwright-results` passed `2/2`.
- Manual Chromium at `1440x900` observed `documentScrollTop=0`, `frameScrollTop=0`, frame `900/900`, `.shell-main` scrolling to `508`, final Save control visible, and Sign out visible.
- Manual Chromium at `375x812` observed the logout failure alert focused below the sidebar, focus ring within viewport, no intersecting enabled controls, and no horizontal overflow.
- The only browser console errors were expected fixture responses from unauthenticated session bootstrap and synthetic logout failure.

## Blocking Findings and Evidence Boundaries

1. Exact-tip run `33919997302` is completed `failure`; Linux package job `101176805555` failed with p7zip `E_FAIL`/exit `2`, and split-volume verification did not run. Approval requires a successful exact-tip run or equivalent accepted authority proving that required legacy package path.
2. The current `F3-manual-visual.md` remains a historical `REJECT` for `67620f5`; exact-tip source, focused E2E, manual browser measurements, and task-19 remediation evidence close its two product findings, but the final-wave record remains internally inconsistent until F3 is rerun and replaced.
3. The `dev` push did not create a GitHub Release or publish Docker Hub images, and GitHub reports no retained downloadable artifacts for this run. No production publication is claimed.

VERDICT: REJECT
