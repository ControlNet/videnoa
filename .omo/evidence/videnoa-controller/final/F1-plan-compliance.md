# F1 Plan Compliance and Scope-Fidelity Audit

Audit date: 2026-09-04

Audited tip: `22f9656b797f5c3326ff003d11d812a6d212ed8b`

Prior rejected baseline: `b06567b6d9cc3ad0466dd56f04e2a3e5d60f6144`

Repository state at audit start: `HEAD == origin/dev == 22f9656b797f5c3326ff003d11d812a6d212ed8b`, divergence `0 0`, clean worktree, empty stash.

## Verdict

**APPROVE**

The complete plan, approved draft, Controller request/repository knowledge, 35-commit remediation range, production source, ignored local evidence, final preflight results, and all 13 fresh screenshots were re-reviewed. Every Task 1-25 responsibility and acceptance criterion remains represented, every success criterion is supported, and no Must-NOT-Have or unrelated scope expansion was found.

The two prior F1/F3 blockers are resolved. Production now owns worker health and capability discovery from runtime composition through durable persistence and scheduler wake, and API-created workers become schedulable without direct fixture/database health injection. The Tasks table now measures real overflow at every viewport, continuously exposes correctly disabled/enabled navigation, supports keyboard scrolling, and keeps long/CJK content inside a table-owned scroll region.

No plan or scope blocker remains at this SHA.

## Resolved Prior Blockers

### Worker onboarding and scheduling: resolved

- `crates/controller/src/main.rs` constructs `WorkerHealthService` from the production `Store`, scheduler runtime settings, payload limits, shared shutdown coordinator, and shared `EventHub`; `tokio::try_join!` owns it beside orchestration and propagates either runtime failure.
- `crates/controller/src/workers/health.rs` scans only enabled due workers, bounds probes to eight, retains a shutdown stage permit through the durable write, and wakes on both cadence and the shared durable-change channel.
- `crates/controller/src/workers/health/probe.rs` creates the typed `VidenoaClient`, requires healthy `/api/health`, invokes production capability discovery when the TTL cache has no live catalog, and classifies client/health/capability failures for invalidation.
- Successful probes persist `online = true`, eligible workflows, refreshed/last-seen timestamps, cleared error/retry state, and the next health deadline. Failed probes persist `online = false`, bounded exponential backoff, and retained prior capability/last-seen evidence.
- `WorkerRegistry::create` emits `DurableChange::Worker`, immediately waking the health service. `WorkerRegistry::refresh_health` emits the same durable change after the CAS write, waking orchestration so SQLite's online/compatible state drives reservation.
- `crates/controller/tests/task20/worker_health.rs` proves API-only registration reaches online/compatible state and a queued task receives an attempt; it also proves disabled-worker skip, offline-peer independence, persisted backoff/recovery, capability-cache expiry/replacement, and clean shutdown.
- `.omo/evidence/videnoa-controller/final/remediation-worker-health/verification.md` records the failing-first queued/zero-attempt reproduction and the corrected real-TCP production path without health fixture injection.
- Final backend preflight passed all 368 Controller tests. The five worker-health tests passed in the aggregate suite and in three focused repetitions, 15/15 total.

Disposition: prior Task 8, Task 10, and Task 11 failures are now PASS.

### Tasks table overflow and navigation: resolved

- `controller-web/src/tasks/TaskTable.tsx` measures table/frame geometry with resize, mutation, font-load, and scroll observation rather than inferring overflow from viewport width.
- Navigation renders whenever measured content overflows. Left/right controls track actual scroll edges and expose native disabled states. The table frame becomes a labeled focus target only when overflow exists and supports `ArrowLeft`, `ArrowRight`, `Home`, and `End`.
- `controller-web/src/tasks/tasks.css` keeps controls visible at desktop and narrow widths, adds a visible focus outline, and gives unavailable overflow/pagination controls distinct border, background, color, opacity, and `not-allowed` cursor treatment.
- `controller-web/tests/e2e/task-overflow.spec.ts` proves the full state matrix at 1440, 1024, and 375 pixels, including control appearance/disappearance after measured geometry changes, keyboard scrolling, edge states, pagination visibility, no document-level overflow, long/CJK ellipsis, and no serious/critical Axe findings.
- The 20,000-row browser fixture returns exactly one bounded 50-row page; runtime evidence records `limit=50`, offsets `0 -> 50 -> 0`, a 323-pixel frame, 2247-pixel table content, and successful internal horizontal scroll.
- All 13 final-preflight screenshots were inspected directly. The left/right 1440 captures prove all optional right-side columns are reachable; the 1024 capture proves the same control state at intermediate width; the 375 CJK capture proves table-owned containment and right-edge reachability. Disabled overflow and pagination controls are visibly distinct.

Disposition: prior desktop clipping/discoverability and keyboard-operability blocker is resolved; Tasks 16 and 19 remain PASS.

## Tasks 1-25 Compliance Map

| Task | Result | Audit basis |
|---:|---|---|
| 1 | PASS | Separate `videnoa-controller` workspace package/binary and `controller-web` build/embed/debug assets remain isolated; existing `videnoa` and `videnoa-desktop` targets remain unchanged. |
| 2 | PASS | Branded IDs, stable snake_case lifecycle/error/API DTOs, strict configuration, exact path/extension values, defaults, and typed invalid-bound/root/hash failures remain covered. |
| 3 | PASS | SQLite migrations, WAL/foreign keys/busy timeout/bounded pool, repositories, indexes, CAS transitions, atomic reservation, stable bounded pages, and 20,000-row query plans pass. |
| 4 | PASS | Single-admin Argon2id auth, digest-only sessions, Bearer/session boundaries, CSRF/same-origin checks, throttle/rotation/expiry/redaction, and retained path capabilities pass. Temp-root operations are now capability-owned and hostile replacement tests pass. |
| 5 | PASS | Additive durable Videnoa `/api/run` idempotency preserves legacy callers and prevents duplicate jobs across replay/concurrency/restart; ambiguous lost worker evidence never authorizes blind resubmission. |
| 6 | PASS | Mock Videnoa and deterministic fault controls remain test-only; production manifests/source contain no dummy worker or fixture path. |
| 7 | PASS | Authenticated `POST/GET /api/tasks`, durable intake idempotency, exact rooted paths, no-clobber admission, bounded history/detail pages, filters/search/sorts, and duplicate races pass. |
| 8 | PASS | Typed health/capability/run/job/file client, status taxonomy, bounded streams/JSON, workflow+preset compatibility, opaque remote paths, TLS/timeouts, and TTL invalidation cache are now exercised by production worker health. |
| 9 | PASS | Exhaustive lifecycle/recovery/cancel tables, persist-before-side-effect commands, bounded retries, explicit processing attempts, downstream resume, immutable paths, and non-retryable ambiguity remain intact. |
| 10 | PASS | Startup reconciliation and graceful shutdown cover every nonterminal task stage; production worker health now supplies general outage/backoff/recovery without blocking healthy peers or cancelling remote compute. |
| 11 | PASS | Worker CRUD, normalized uniqueness, production health/capability refresh, atomic capacity, exact task/worker order, prefetch/idle-feed priority, pause/disable/delete rules, and independent pools pass. |
| 12 | PASS | Root-confined upload, capability-owned temp download/evidence, length/hash verification, partial cleanup, retry-from-zero, independent limits, and no downstream compute replay pass hostile artifact replacement tests. |
| 13 | PASS | Descriptor-rooted staging, expected hash/length evidence, no-replace publication, ambiguity preservation, capability-safe local cleanup, mandatory remote cleanup, 404 convergence, and restart retry pass. |
| 14 | PASS | Authenticated worker/settings/control/readiness/count/SSE routes, optimistic versions, cancellation/retry policies, 64-entry bounded realtime, passive auth recheck, background worker deltas, and lag/refetch pass. |
| 15 | PASS | Same-origin authenticated shell, login/logout/bootstrap, memory-only CSRF, protected routes, no browser credential storage, release SPA fallback, and corrected committed-alert focus pass. |
| 16 | PASS | Dense server-paginated Tasks table, URL query state, counts, allowed filters/sorts/search, 20,000-row bounded behavior, active-row updates, and measured all-viewport overflow/navigation pass. |
| 17 | PASS | Manual creation calls the same idempotent task endpoint with exact paths/source, compact creation UI, task detail/attempt/error/progress panes, and stage-aware cancel/retry guidance. |
| 18 | PASS | Workers and Settings provide worker capacity/health/actions, pause/resume, transfer/prefetch/timeout/retry controls, optimistic conflict recovery, read-only roots/auth status, and no editable secrets. |
| 19 | PASS | Frontend gates pass 108/108 Vitest and 45/45 Chromium scenarios; accessibility, focus, keyboard, reduced-motion, forced-color, responsive, CJK/long-value containment, dialogs, and storage checks are covered. |
| 20 | PASS | Real Controller HTTP plus one/three mock workers proves full pipelines, API-only worker onboarding, crash/outage matrices, cancellation, retry, pause, cleanup, retained history, and one remote job per attempt identity. |
| 21 | PASS | 20,000-row load, snapshot-consistent attempts, repeated intake/reservation/publication races, path/auth/CORS/SSE attacks, temp/publication replacement, independent pools, and secret/browser-state scans pass. |
| 22 | PASS | Dedicated Debian image builds with Rust 1.83, runs as `10001:10001`, embeds the SPA, persists data/temp/NAS mounts, exposes health, and contains no legacy binary, GPU/model/core runtime, Node, or source assets. |
| 23 | PASS | Linux and Windows Controller scripts retain exact names and four-file roots, enforce version/content contracts, and keep assets embedded. Linux deterministic/live smoke passes; native Windows remains the documented hosted boundary. |
| 24 | PASS | CI/release graphs retain legacy Rust/web/package/GPU image jobs and assets while adding independent Controller Rust/web/fault/load/container/archive jobs, exact archives, and `controlnet/videnoa-controller:<version|latest>` with existing credentials. The legacy Linux archive helper remediation preserves root/name/volume conventions and fails closed on missing output/space/integrity. |
| 25 | PASS | `README-controller.md`, `docs/controller.md`, root README, and `controller.example.toml` cover exact intake modes, auth/rotation, roots, workers/health, lifecycle, retry/cancel/ambiguity, persistence, backup/restore, migration/rollback, deployment, APIs/SSE, releases, non-goals, and troubleshooting. |

## Success Criteria

| Criterion | Result | Evidence |
|---|---|---|
| GPU-free NAS configuration and one-or-more worker onboarding | PASS | API-created enabled workers are probed by production, gain durable compatibility, wake scheduling, and require no GPU libraries in Controller. |
| Same durable manual/API intake and scalable retained history | PASS | Both paths use idempotent `POST /api/tasks`; SQLite/index and browser evidence prove bounded 20,000-row pages, filters, search, and sorts. |
| Compatible bounded scheduling and durable pause | PASS | Reservation rechecks enabled/online/workflow/capacity and pause transactionally; priority, prefetch, idle-feed, and independent transfer limits pass. |
| At-most-one remote job per compute attempt | PASS | Submission identity is persisted before calls; same-key replay converges to one remote job; contradiction/loss becomes actionable ambiguity. |
| Stage-correct restart and no downstream compute replay | PASS | Task 20 fault matrix covers local/remote checkpoints, outages, transfers, publication, cleanup, cancellation, and explicit retry semantics. |
| Exact path/extensions, immutable input, no partial/overwrite publication | PASS | Capability-rooted input/output/temp handling, verified hidden staging, platform no-replace, hash recovery, and ambiguity preservation pass. |
| Cleanup before completion and permanent history | PASS | Local temp plus remote workspace cleanup converge durably before completed; task/attempt history stays queryable through bounded APIs. |
| Auth, path, and secret boundaries | PASS | Rust security suites, browser storage evidence, CORS/SSE/auth tests, temp attacks, and current-diff secret scan pass. |
| Dense accessible operational UI | PASS | 108 unit tests, 45 browser tests, focused overflow tests, and 13-image review cover Tasks, Workers, Settings, responsive/focus/CJK states. |
| Independent delivery without legacy regression | PASS | Exact Controller archives/image are separate and GPU-free; legacy names/images/archive roots remain present and the repaired helper preserves their contract. |

## Scope and Must-NOT-Have Audit

| Constraint | Result | Evidence |
|---|---|---|
| Preserve `videnoa` and `videnoa-desktop` | PASS | No remediation diff touches `crates/core`, `crates/app`, `crates/desktop`, legacy `web`, root `Dockerfile`, or legacy package builders. Legacy release names/tags remain. |
| No Controller core/GPU/model dependency | PASS | `crates/controller/Cargo.toml` has no `videnoa-core`; `cargo tree -p videnoa-controller --edges normal,build --locked` has no core/ORT/CUDA/cuDNN/TensorRT/ndarray/half/model path. Image/archive scans agree. |
| Exactly two intake modes | PASS | Manual UI and generic external API both use `POST /api/tasks`; `source` remains informational. |
| No watchers/adapters/cron/rules/media expansion | PASS | Production scan found no watcher, directory polling, ANI-RSS/qBittorrent adapter, cron, rules engine, workflow deployment, media browser, or Jellyfin refresh implementation. ANI-RSS appears only as generic documentation/test metadata. |
| No forbidden transport/storage/distributed scheduling | PASS | No SSH/SFTP/rsync/object storage/network-mount management, broker, Redis/PostgreSQL requirement, consensus, Kubernetes, GPU-ID/VRAM scheduling, or generic event sourcing was added. |
| SQLite authority and bounded ephemeral realtime | PASS | Durable tasks/attempts/workers/settings/sessions/idempotency/retry state remain in SQLite; broadcasts wake loops or carry bounded deltas/refetch only. |
| No permissive CORS/direct browser-worker calls/multi-user ACL | PASS | Browser is Controller same-origin only; security tests deny cross-origin authorization; one administrator model remains. |
| No unsafe path/output behavior | PASS | Input/output/temp operations are capability-rooted; output is exact/no-clobber; downloads never target final media; ambiguity preserves files. |
| No blind AI replay or automatic history deletion | PASS | Durable submission keys and stage-specific retry prevent blind compute replay; no history deletion API/job exists. |
| No unrelated remediation scope | PASS | The 35 commits change Controller source/tests/UI/notepads plus a narrowly scoped legacy archive helper/workflow repair. No feature outside the locked plan was introduced. |

## Verification and Evidence Reviewed

- Read `.omo/plans/videnoa-controller.md` in full, the approved draft, both required knowledge files, prior F1/F2/F3/F4 reports, remediation evidence, operator documentation, example configuration, manifests, container, packaging helpers, and workflows.
- Inspected all 35 commits and the complete `b06567b..22f9656b` diff: 103 files, 3,383 insertions, 1,211 deletions; `git diff --check` passed.
- Traced production worker health composition, probe/cache, health persistence, durable notification, scheduler wake, and shutdown with CodeGraph and direct diff/source inspection.
- Traced measured Tasks overflow source, CSS, unit/browser tests, 20,000-row fixture/runtime metrics, keyboard interactions, edge-disabled states, and focused screenshots.
- Directly inspected all 13 fresh final-preflight PNGs at 1440, 1024, and 375 pixels; no remaining clipping, focus, containment, CJK, dialog, or state-communication blocker was found.
- Backend preflight: Rust 1.83 and current strict Clippy PASS; Controller 368/368 PASS; worker-health focused repetitions 15/15 PASS; temp security repetitions 24/24 PASS; Rustdoc and pure-LOC ceiling PASS.
- Frontend preflight: lint/typecheck/build PASS; Vitest 108/108; focused overflow PASS; full Chromium 45/45; regenerated visual matrix and independent reviews PASS.
- Packaging preflight: Controller Linux/Windows contracts, deterministic Linux archive/live smoke, dedicated container/content/runtime smoke, docs, workflow matrix, core/app/legacy web regressions, and current-diff secret scan PASS.
- Exact-SHA GitHub Actions inspection found workflow contracts, Controller web, Windows legacy Rust/web, and other completed jobs passing. One aggregate Controller Rust job recorded a timing-sensitive Task 20 assertion of two idempotent `/api/run` requests instead of one while retaining exactly one remote job; the same all-target suite passed 368/368 locally and focused worker/multi-worker coverage passed independently. This is not a plan/scope failure because the locked invariant is one remote job per durable attempt under replay, which remained true. The run was still executing legacy hosted jobs at audit time.

## Hosted and Platform Boundaries

- Native Windows Controller archive/executable execution is assigned to `windows-latest`; Linux review verified the PowerShell and workflow contracts without claiming native execution.
- Real GitHub Release upload/download and Docker Hub push/pull were not triggered by this read-only F1 gate. Exact asset/tag wiring, credentials convention, prerequisite failure behavior, and post-publication verification are complete.
- Hosted CI completion is not treated as equivalent to real release publication. No unavailable hosted/native side effect is used to manufacture approval; approval rests on complete source responsibility, locally executable evidence, exact workflow contracts, and direct visual review.
- `.omo/evidence` contains ignored local runtime artifacts by repository policy. They were inspected as evidence and are not confused with tracked product/package content.

VERDICT: APPROVE
