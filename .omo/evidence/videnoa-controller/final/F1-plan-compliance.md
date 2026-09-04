# F1 Plan Compliance and Scope-Fidelity Audit

Audit date: 2026-09-04

Audited tip: `094d87833ebbc55c8f27857cd70d85fc5cc91afe`

Prior audited tip: `22f9656b797f5c3326ff003d11d812a6d212ed8b`

Hosted authority: [GitHub Actions run 33873102244](https://github.com/ControlNet/videnoa/actions/runs/33873102244)

## Decision

**REJECT**

The complete plan, approved draft, Controller request and repository knowledge, implementation history from Controller inception through the audited tip, current source, tests, task evidence, remediation evidence, packaging and documentation contracts, and exact-tip hosted run were reviewed. Product, lifecycle, security, packaging, documentation, and scope boundaries are substantively represented, but the required full Playwright gate is not independently reproducible at this exact tree. Task 19 and the dense accessible UI success criterion therefore do not currently satisfy their pass requirements.

The audited implementation is additive and separately named. Existing `videnoa`, `videnoa-desktop`, GPU image, web application, and legacy archive contracts remain present. `videnoa-controller` has an independent crate, frontend, image, archives, release names, and documentation, with no dependency or packaged-content path to `videnoa-core`, ONNX Runtime, CUDA, cuDNN, TensorRT, models, or GPU runtime.

One F1 blocker remains at this SHA: a fresh complete `npm run test:e2e` execution returned 46 passed and 1 failed. `task-overflow.spec.ts` observed the right-edge navigation control enabled and fully opaque after layout growth, when the native and computed unavailable states were required to remain disabled, `opacity: 0.65`, and `cursor: not-allowed`. A focused three-repeat rerun passed 9/9, establishing timing/layout sensitivity rather than disproving the full-suite failure. The exact-tip hosted pass is valid evidence, but it does not erase a fresh contradictory execution of the same required acceptance gate.

## Blocking Finding

### F1-B1: Required browser overflow state is timing-sensitive

The complete local browser command required by the plan was executed against the audited tree:

```text
cd controller-web && npm run test:e2e
46 passed, 1 failed
failed: tests/e2e/task-overflow.spec.ts:44
```

The failing scenario first moved the task table to its effective right edge, then expanded the table geometry by 20 pixels. The implementation is intended to retain right-edge anchoring through `ResizeObserver`/`MutationObserver` callbacks. In the failed full-suite execution, the right control instead became available and had `opacity: 1` and `cursor: pointer`; the assertion required the native disabled state and unavailable styling.

Focused repetition did not reproduce the failure:

```text
npx playwright test tests/e2e/task-overflow.spec.ts --project=chromium --workers=1 --repeat-each=3
9 passed
```

This is a determinism failure, not evidence that the complete gate passed. Task 19 acceptance explicitly requires Playwright to pass at desktop/narrow widths, and its QA explicitly covers responsive overflow/sticky controls. The dense accessible operational UI success criterion likewise requires Playwright and responsive checks to pass. Both remain blocked until the full suite is stable and green.

## Exact-Tip Authority

At audit start, `HEAD == origin/dev == 094d87833ebbc55c8f27857cd70d85fc5cc91afe`, branch divergence was `0 0`, the worktree contained no pre-existing product change, and the stash was empty. The only later observed worktree modification was a concurrent final-review edit to `F4-release-regression.md`; it was not modified or used as proof by this F1 audit.

Run `33873102244` is authoritative because its `headSha` exactly equals the audited tip. It completed with conclusion `success`. All 14 jobs passed:

| Hosted job | Result |
|---|---|
| Workflow contracts | PASS |
| Web build check (Ubuntu) | PASS |
| Web build check (Windows) | PASS |
| Rust tests (Ubuntu) | PASS |
| Rust tests (Windows) | PASS |
| Controller Rust quality and tests | PASS |
| Controller web quality and E2E | PASS |
| Package smoke (Linux) | PASS |
| Package smoke (Windows) | PASS |
| Docker build smoke | PASS |
| Controller fault and load suites | PASS |
| Controller archive smoke (Linux) | PASS |
| Controller archive smoke (Windows) | PASS |
| Controller image and content smoke | PASS |

The prior F1/F2/F3/F4 reports at `22f9656` are stale and do not transfer approval or rejection to this audit. Their material runtime findings are superseded by current source and exact-tip evidence: production SSE observes the shutdown coordinator, durable submission ownership prevents same-generation duplicate `/api/run` requests, restart takeover replays the same key, and the exact-tip hosted Controller Rust plus fault/load jobs pass the repaired Task 20 suite.

The final contention commits `d4d6b97`, `d69bc64`, and `b390cde` change only `crates/controller/tests/task20/**`. They serialize process-heavy fixtures, strengthen coherent assertions and diagnostics, and prevent a crashed cleanup future from committing after restart. They do not alter production Controller, frontend, packaging, workflow, or legacy-product behavior. Commits `54d8b08` and `094d878` are notepad documentation only.

## Tasks 1-25 Compliance Map

| Task | Result | Exact-tip audit basis |
|---:|---|---|
| 1 | PASS | Separate workspace package/binary and `controller-web` build/embed/debug assets exist; existing application and desktop targets remain named and buildable. |
| 2 | PASS | Branded IDs, stable snake_case lifecycle/error/API DTOs, exact paths/extensions, strict configuration, locked defaults, and typed invalid configuration failures are implemented and tested. |
| 3 | PASS | Six SQLx migrations, WAL/foreign keys/busy timeout/bounded pool, repositories, indexes, CAS transitions, atomic reservation, stable bounded pages, and 20,000-row query plans pass. |
| 4 | PASS | Single-admin Argon2id auth, digest-only sessions, Bearer/session boundaries, CSRF/same-origin checks, throttling, rotation/expiry, redaction, and descriptor-rooted path capabilities pass hostile tests. |
| 5 | PASS | Additive Videnoa `/api/run` idempotency preserves unkeyed callers and converges sequential, concurrent, lost-response, and restart replay to one remote job per key. |
| 6 | PASS | Real-TCP mock Videnoa, request journals, and deterministic fault controls are test-only; production dependency and content checks contain no fixture path or fake success behavior. |
| 7 | PASS | Authenticated idempotent task creation plus bounded list/detail APIs preserve exact rooted paths, reject collisions, support allowlisted filters/search/sorts, and withstand duplicate races. |
| 8 | PASS | Typed health/capability/job/file client, bounded JSON/streams, workflow and preset compatibility, opaque remote paths, status taxonomy, timeouts, TLS URLs, and TTL invalidation are implemented. |
| 9 | PASS | Exhaustive lifecycle, recovery, cancellation, retry, persist-before-side-effect, attempt replacement, downstream resume, and ambiguity policies are represented by typed state transitions and tests. |
| 10 | PASS | Startup reconciliation and coordinated shutdown cover every nonterminal stage; health outages retain assignments, back off, spare healthy peers, and never cancel unknown remote compute. |
| 11 | PASS | Worker CRUD, production health refresh, capability persistence, exact task/worker order, atomic capacity, prefetch, idle-feed precedence, persisted pause, and independent transfer limits pass. |
| 12 | PASS | Root-confined streamed upload/download, exact length/hash evidence, partial cleanup, retry from zero, independent pools, and no downstream compute replay pass transfer and replacement attacks. |
| 13 | PASS | Hidden task-owned staging, expected hash/length, platform no-replace finalization, ambiguity preservation, local cleanup, remote cleanup, `404` convergence, and restart retry pass. |
| 14 | PASS | Authenticated worker/settings/control/readiness/count/SSE routes, optimistic versions, cancellation/retry policy, bounded deltas, passive auth recheck, lag/refetch, and worker updates pass. |
| 15 | PASS | Same-origin authenticated shell, login/logout/bootstrap, memory-only CSRF, protected routes, no browser credential storage, embedded SPA fallback, and focus behavior pass. |
| 16 | FAIL | Dense server-paginated history and bounded behavior are implemented, but the fresh full browser suite observed a wrong right-edge navigation state after measured layout growth. |
| 17 | PASS | Manual creation uses the same `POST /api/tasks` contract and durable idempotency as external intake; detail, attempt, progress, error, cancellation, and retry guidance are present. |
| 18 | PASS | Workers and Settings expose capacity, health, actions, pause, transfer/prefetch/timeout/retry controls, optimistic conflict handling, and read-only roots/auth state without editable secrets. |
| 19 | FAIL | Hosted Chromium passed, but a fresh complete local Playwright run returned 46/47; the required responsive overflow control state is timing-sensitive. |
| 20 | PASS | Real Controller HTTP with one and three workers proves full pipelines, all required crash boundaries, outages, pause, cancellation, retry, cleanup, retained history, exact request counts, and one remote job per attempt key. |
| 21 | PASS | 20,000-row load, snapshot consistency, repeated intake/reservation/publication races, filesystem attacks, auth/CORS/SSE attacks, resource isolation, and secret/storage scans pass. |
| 22 | PASS | Dedicated non-root Debian image embeds the SPA, persists state, passes health/error smokes, and contains no legacy binary, source assets, Node, models, core, or GPU runtime. |
| 23 | PASS | Linux and native Windows hosted packaging produce exact independently named four-file roots, enforce version/content contracts, and preserve existing package layouts. |
| 24 | PASS | Exact-tip CI proves Controller and existing products. Release dry-run/contracts enumerate all legacy and Controller assets/tags, reject missing or forbidden outputs, preserve the existing credential scheme, and require complete publication before release success. |
| 25 | PASS | Root README, archive guide, operations guide, and example configuration cover setup, intake, auth, roots, workers, lifecycle, recovery, backup/restore, upgrade/rollback, deployment, API/SSE, releases, non-goals, and troubleshooting. |

## Success Criteria

| Criterion | Result | Evidence |
|---|---|---|
| GPU-free NAS setup and worker onboarding | PASS | Production health probes API-created workers, persists compatibility, wakes scheduling, and requires no Controller GPU library. |
| Equivalent durable manual/API intake and scalable history | PASS | Both paths call idempotent `POST /api/tasks`; indexed SQLite and browser tests prove bounded retained history. |
| Compatible bounded scheduling and durable pause | PASS | Reservation transactionally rechecks enabled/online/workflow/capacity/pause state; ordering and all configured limits pass. |
| At most one remote job per compute attempt | PASS | Submission keys and ownership are durable before calls; replay converges to one job and ambiguous evidence never authorizes a new attempt. |
| Correct restart stage with no downstream compute replay | PASS | Task 20 covers all specified local and remote crash boundaries, outages, cancellation, retry, and cleanup convergence. |
| Exact immutable paths and no partial/overwrite publication | PASS | Capability-rooted input/output/temp handling, verified staging, no-replace finalization, and ambiguity preservation pass adversarial tests. |
| Cleanup before completion and permanent queryable history | PASS | Local and remote task workspaces converge before `completed`; retained task/attempt pages remain bounded and indexed. |
| Auth, path, CORS, SSE, and secret boundaries | PASS | Backend security suites, browser storage checks, content scans, and exact-tip hosted gates pass. |
| Dense accessible operational UI | FAIL | Unit/build and hosted browser evidence pass, but the fresh complete Playwright gate failed the right-edge overflow state after layout growth. |
| Independent archives/image without legacy regression | PASS | Linux/Windows archive and image smokes pass at the exact tip; release contracts preserve all existing names/layouts and add only separate Controller outputs. |

## Scope and Must-NOT-Have Audit

| Constraint | Result | Audit basis |
|---|---|---|
| Preserve `videnoa`, `videnoa-desktop`, legacy web/image/archives | PASS | Existing workspace members, binaries, Dockerfile, package scripts, image tags, archive names, roots, and hosted regression jobs remain. |
| No Controller core/GPU/model dependency or packaged content | PASS | Manifest/tree, archive linkage/content, and image inspections reject core, ORT, CUDA, cuDNN, TensorRT, NVIDIA, model, and cache paths. |
| Exactly manual GUI and generic external API intake | PASS | Both use authenticated `POST /api/tasks`; `source` remains informational and no semantic adapter was added. |
| No watchers, polling discovery, ANI-RSS/qBittorrent semantics, cron, rules, workflow deployment, media browser, or Jellyfin refresh | PASS | No such production subsystem exists; documentation mentions generic callers only. |
| No SSH/SFTP/rsync, mount management, object storage, resumable protocol, external database, broker, consensus, Kubernetes, or GPU scheduling | PASS | Remote execution and files use Videnoa HTTP APIs; durable authority is local SQLite; scheduler models worker slots rather than GPUs. |
| No multiple users, roles, accounts, or ACL | PASS | Authentication remains one administrator password with session and Bearer modes. |
| No permissive CORS, browser-to-worker calls, unbounded history, card-heavy history, whole-table realtime, queue-wide pre-upload, or shared transfer semaphore | PASS | Same-origin Controller routes, bounded SQL pages, active deltas/refetch, dense table UI, bounded prefetch, and independent pools are tested. |
| No guessed extension, input mutation, overwrite/rename fallback, final-path download, or deletion of ambiguous files | PASS | Exact extensions and paths persist; input is reopened read-only; download remains under temp; no-replace and ambiguity rules fail closed. |
| No blind AI replay or automatic history deletion | PASS | Durable keys, ownership, remote ambiguity, and stage-specific retry prevent replay; completed task/attempt deletion is not implemented. |
| No unrelated final remediation scope | PASS | Tip contention changes are Task 20 test-only; notepad commits are documentation-only; no unrelated feature was introduced. |

## Evidence Boundaries and Residual Risk

- Real GitHub Release creation and Docker Hub publication were not triggered by the `dev` push and are not claimed. Task 24 acceptance requires the release dry-run, complete asset/tag graph, negative mutation enforcement, and existing credential convention; those pass. The release workflow itself gates success on both Controller archives, both Controller image tags, preserved legacy outputs, GitHub asset upload, and post-publication verification.
- Native Windows execution is proven by the exact-tip hosted Windows Rust and Controller archive jobs. The Linux review does not claim local PowerShell execution.
- Migration 0006 has exact-tip fresh, migration-5 upgrade, idempotency, and atomic failure coverage. Backup/restore/rollback is intentionally an operator full-snapshot procedure rather than a down-migration; documentation requires restoring matched Controller and worker state and never running an older binary against a newer database.
- Ignored local evidence supports diagnosis but is not treated as tracked product content. Findings rely on current source/tests, exact-tip hosted results, and explicit evidence provenance rather than stale final reports.
- A fresh local Task 20 target did not finish inside a 600-second review timeout. This is not classified as a product failure because the exact-tip hosted Controller Rust and separately gated fault/load jobs both completed the full Task 20 target successfully, but local execution time remains a reproducibility risk.

## Verification Reviewed

- Read the complete plan, approved draft, both required knowledge files, task/final evidence, remediation records, notepads, manifests, source, tests, frontend, container, packaging scripts, workflows, documentation, and example configuration.
- Traced intake through SQLite, durable notifications, orchestration, scheduler admission, upload, keyed submission ownership, polling, download, verification, publication, cleanup, cancellation, retry, and recovery.
- Verified the complete implementation history and exact tip; the final Task 20 contention range changes only test infrastructure and scenarios.
- Verified run `33873102244` directly: exact `headSha`, completed `success`, and all 14 jobs successful, including both Controller Task 20 executions and native Windows archive smoke.
- Reconciled independent goal/compliance, implementation-quality, security-boundary, QA/evidence, and repository-context reviews. Implementation-quality, security, and repository-context lanes found no architecture or scope blocker. The evidence lane's credentialed-publication concern does not match Task 24's explicit dry-run acceptance and read-only audit boundary. The goal/compliance lane did, however, produce a concrete fresh full-suite Playwright failure; this report adopts that reproducible acceptance-gate conflict as blocking.

VERDICT: REJECT
