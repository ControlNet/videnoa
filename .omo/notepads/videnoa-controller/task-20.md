# Task 20 Notepad

## 2026-09-03 failing-first pipeline proof

- A Task 20-only real-TCP Controller fixture now drives authenticated `POST /api/workers`, `POST /api/tasks`, and `GET /api/tasks/{id}` against real SQLite and filesystem capabilities while the existing persistent mock Videnoa instances run on real TCP.
- One-worker proof deterministically reaches `Queued`, zero attempts, zero `/api/run` requests, and zero remote jobs, then times out at `AfterRunPersistedBeforeResponse`.
- Three-worker proof deterministically reaches three queued tasks, zero attempts, zero run requests across all workers, and zero remote jobs, then times out at the same named checkpoint.
- The regression intentionally remains failing. Shared production Rust was not edited in this parallel pass.

## Exact production blocker

- `crates/controller/src/main.rs` performs startup reconciliation and transfer dispatch once, then serves HTTP. It never launches a normal orchestration loop that calls `Scheduler::reserve_next`, admits uploads, submits keyed `/api/run`, polls jobs, or dispatches download/verify/publish/cleanup advancement.
- `TransferExecutor::dispatch_recovery` explicitly ignores `RecoveryCommandKind::Submit` and `RecoveryCommandKind::Poll`, so restart cannot resume those stages through the production composition root either.
- Required sequential fix: add a production orchestration runtime owned by the composition root. It must consume durable wakeups, repeatedly select and atomically reserve eligible work, execute upload, keyed submit/reconciliation, long poll, download, verify, publish, local cleanup, and remote cleanup, and hold capacity with RAII across each durable attempt. Startup recovery must dispatch or enqueue `Submit` and `Poll` commands rather than dropping them.
- The runtime needs a deterministic shutdown/restart handle and test checkpoint observer so Task 20 can kill/restart the Controller at each existing irreversible-effect checkpoint without sleeps.

## Executed evidence

- Existing deterministic suites executed in this pass: mock Videnoa 26/26, Task 11 14/14, Task 12 19/19, Task 13 24/24, Task 14 28/28.
- Task 20 matrix tests pass: 15/15 Controller boundaries mapped and 11/11 outage classes mapped.
- Task 20 one-worker and three-worker full-pipeline tests fail at the first missing runtime dispatch, preserving the regression before correction.
- Task 20 Rust files pass direct rustfmt check, strict all-target/all-feature Controller Clippy, and the eight-file no-excuse size/type scan.

## 2026-09-03 orchestration runtime implementation

- Added production `Orchestrator` ownership in `main.rs`; removed the incomplete one-shot startup dispatch. The runtime performs atomic reservation, concurrent per-task stage execution, durable rescans, bounded poll cooldowns, and EventHub wakeups.
- Added migration `0005_worker_scoped_remote_job.sql` because remote job identity is scoped to a worker API. The prior global unique index prevented valid three-worker execution when each independent worker returned the same first job ID.
- Added migration-directory build tracking so newly added SQLx migrations are re-embedded without a manual clean rebuild.
- Extended shutdown coordination to count stage lifetime in addition to durable writes and added an executed Task 20 drain assertion.
- One-worker real-TCP pipeline passes through upload, keyed submission/replay-safe persistence, poll, download, verification, no-clobber publication, local cleanup, and remote cleanup with one retained attempt and one remote job.
- Three-worker real-TCP pipeline passes with all three slots active and one remote job/request per assigned worker. The test now resolves actual durable `worker_id` assignments rather than assuming fixture insertion order.
- Verification passed: Task 20 5/5, mock shutdown 4/4, mock Videnoa 26/26, Task 11 14/14, Task 12 19/19, Task 13 24/24, Task 14 28/28, strict all-target Clippy, and all-target `cargo check`.
- Remaining qualification gap: `fault_matrix.rs` and `outage_matrix.rs` still assert named mappings rather than launching every Controller restart/outage scenario in the Task 20 target. Existing Task 10-13 regressions execute those component boundaries, but the Task 20 matrix itself still needs process-level restart/outage execution before final evidence can claim the complete matrix.

## 2026-09-03 executable crash and outage matrix

- Replaced both mapping-only tests with real-TCP scenarios. Task 20 now aborts and recreates Controller HTTP plus `Orchestrator` against the same SQLite and filesystem roots at all 15 required crash boundaries.
- Added no-op-by-default transfer observer points for verified-download-before-CAS and destination staging before/after copy, completing the irreversible local boundary surface.
- Reopened a fresh `Store` for each Controller generation so the new `EventHub` owns the generation's durable-change observer.
- Executed all 11 worker outage/restart/pause/retry/cancellation rows through Controller HTTP and persistent mock Videnoa TCP behavior.
- Final verification: Task 20 11/11, Task 12 19/19, Task 13 24/24, strict all-target Clippy, Controller build, rustfmt check, and real-surface crash/recovery assertions all pass.

## 2026-09-03 distinct crash-boundary correction

- Replaced the aliased upload/submit proof with `UploadCompleted` after the durable `Staged` CAS and `BeforeRemoteSubmit` immediately before `VidenoaClient::run`; both assert one retained attempt and zero `/api/run` requests before crash.
- Replaced the aliased remote-completion/download proof with `RemoteCompletionPersisted` after the durable `RemoteCompleted` CAS and mock `MidDownloadBody` after the client has written a positive byte count to `.part`.
- The focused remote restart matrix passes all nine named remote boundaries with deterministic gates and no sleeps.

## 2026-09-03 architecture and evidence rejection remediation

- Added an executed pause-after-staging regression. `Scheduler::allows(DurableAction::Submit)` is now evaluated immediately before `/api/run`, so a persisted pause leaves the same task/attempt in `Submitting` until resume.
- Added graceful HTTP cancellation and a common `main` exit path for server, orchestration, and signal outcomes. Shutdown closes HTTP intake, persists pause, stops stage admission, drains, awaits the surviving future, and returns the original primary failure first.
- Replaced silent stage-error retry with an exhaustive transient/fatal partition and proved a real SQLite stage write failure terminates orchestration.
- Reworked the mock upload gate to stop after a positive strict subset of a 16 KiB request and added `LocalCleanupCompleted` after local deletion/sync but before remote delete.
- Added explicit processing-retry convergence with distinct attempt/submission identities, accepted processing cancellation, exact lost-submit idempotency replay, exact cleanup delete counts, durable capacity release, Controller temp absence, and remote file absence.
- Regenerated three non-empty sanitized worker request journals from the real-TCP multi-worker scenario.
- Verification passed: Task 20 14/14, Tasks 12-14 71/71, strict Controller Clippy, Controller build, rustfmt, and the 30-file no-excuse scan.

## 2026-09-03 independent review closure

- Replaced the check-then-submit pause boundary with a `Store`-shared `RwLock`: submissions hold concurrent read admission through `VidenoaClient::run`, while settings and shutdown pause updates require exclusive write admission.
- Centralized an exhaustive `VidenoaClientError::is_transient` partition and reused it for orchestration stage outcomes and worker health deferral; malformed payload, path, local I/O, endpoint, client-status, conflict, and identity failures now terminate instead of looping.
- Coordinated shutdown now converts an incomplete bounded drain into `ShutdownError::DrainTimedOut` with outstanding stage/write counts.
- Persisted request journals now replace UUID text with `{id}` and redact idempotency-key values without weakening in-memory idempotency assertions.
- Final Task 20 verification passes 16/16, including atomic pause admission, drain-timeout escalation, sanitized evidence, and three-worker concurrent capacity.
- Independent runtime architecture review: PASS. Independent evidence review: PASS after refreshing the 16-test counts.
- Task 21 artifacts remain inherited and untouched because the active scope explicitly forbids modifying them.

## 2026-09-03 task-local remote failure isolation

- Added two real-TCP, two-worker regressions. A submission HTTP 400 now durably fails only its task as non-retryable `remote_submission_failed`; a malformed HTTP 200 poll payload now durably fails only its task as non-retryable `remote_state_ambiguous`.
- Both scenarios prove the same Orchestrator remains alive by completing an unrelated task through upload, keyed submission, polling, download, publication, and cleanup.
- Added a typed recovery boundary that separates task-owned response failures from transient cooldown failures and process-fatal endpoint/local-I/O/configuration failures. Lifecycle persistence errors continue to propagate after acquiring the stage write permit.
- Added scripted `/api/run` response faults to the real-TCP mock and stabilized remote completion after `BeforeRunPersistence` with a bounded, yield-based persistence wait.
- Red phase: both isolation tests failed with `task did not reach Failed` on the original propagation path.
- Final verification passed: Task 20 twice at 18/18, Tasks 12-14 at 71/71, Task 11 at 14/14, library tests at 8/8, strict all-target/all-feature Clippy, Controller build, rustfmt, LSP diagnostics, and the eight-file no-excuse scan.

## 2026-09-03 evidence rejection remediation

- Accepted processing cancellation now deletes the task's remote workspace before committing terminal cancellation; not-found remains idempotent success and transient deletion failures leave cancellation pending for reconciliation.
- The cancellation proof asserts both task and retained attempt are `Cancelled`, attempt count remains one, all worker capacity/activity counters return to zero, run and cancel routes are exactly one, remote jobs/files are absent, and the Controller task workspace is absent.
- Cleanup 404/5xx proof snapshots upload, run, and download counters at one before deletion and asserts they remain unchanged after retry; delete counts are exactly one and two respectively.
- Every local transfer checkpoint now asserts pre-crash task/attempt status, persisted publication evidence, `.part` absence, verified/evidence retention, staging/final visibility, Controller temp cleanup, remote file presence, and an exact unchanged run-route count.
- The plan contains 15 required crash boundaries: nine remote and six local. `StagingVerified`, `LocalCleanupCompleted`, and `RemoteDeleteSucceeded` are three auxiliary recovery checkpoints and are labeled separately in evidence.
- Definite cancellation cleanup rejections are persisted as task-local non-retryable `RemoteCleanup/CleanupFailed` outcomes instead of terminating orchestration; transport/server outages remain retryable and infrastructure/configuration failures remain fatal.
- Verification passed: Task 20 19/19, Tasks 12-14 71/71, Task 11 14/14, library tests 8/8, strict all-target/all-feature Clippy, Controller build, rustfmt, LSP diagnostics, and the five-file no-excuse scan.

## 2026-09-03 downstream cancellation local-cleanup red phase

- Added real-path `Downloading` cancellation coverage that pauses after positive `.part` bytes are written, persists cancellation, crashes, restarts, and observes terminal `Cancelled` with the Controller workspace still present.
- Added real-path `Verifying` cancellation coverage that installs real `.verified` bytes and `.verified.evidence`, persists cancellation across restart, and observes terminal `Cancelled` with both local artifacts still present.
- Exact red command: `cargo test -p videnoa-controller --test task20 cancellation_downstream:: -- --nocapture`.
- Both tests fail for the intended reason at `assertion failed: !workspace.exists()`; remote cleanup and terminal cancellation already converge.
- Root cause: `Reconciler::finish_cancellation` skips the idempotent local deletion and parent-directory sync used by normal Controller cleanup.

## 2026-09-03 downstream cancellation local-cleanup green phase

- Extracted the normal cleanup remove-and-parent-sync operation into one shared idempotent helper and invoked it before cancellation remote deletion and terminal `Cancelled` persistence.
- Added `temp_root` to recovery configuration so startup and live reconciliation use the same task workspace root as transfer cleanup.
- Controller-local cancellation cleanup I/O is a typed retryable recovery error. A failure leaves the original nonterminal status and durable cancellation intent intact, preventing remote deletion or terminal cancellation until local cleanup succeeds.
- Exact focused command passed: `cargo test -p videnoa-controller --test task20 cancellation_downstream:: -- --nocapture` with 2/2 tests.
- Full verification passed: Task 20 21/21, Tasks 12-14 71/71, Task 11 14/14, library tests 8/8, mock Videnoa 26/26, strict all-target/all-feature Clippy, all-target Controller build, rustfmt, LSP diagnostics, `git diff --check`, and the 15-file no-excuse scan.
- Task 21 production, frontend, tests, evidence, and notepad artifacts were not modified by this remediation.

## 2026-09-03 migration-count regression correction

- Red reproduced: `existing_database_migrates_idempotently` failed at line 78 with `left: 5`, `right: 4` after migration 0005 increased the successful migration count.
- Updated only `persistence_migrations.rs`: the test now expects five migrations and asserts `idx_attempts_worker_remote_job` is a partial unique index with ordered columns `worker_id`, `remote_job_id`.
- Green verification passed: persistence migrations 3/3, Task 20 21/21, rustfmt, strict all-target/all-feature Controller Clippy, all-target Controller build, LSP diagnostics, `git diff --check`, and the one-file no-excuse scan.
- Migration 0005, production Rust, and Task 21 artifacts were not modified.
