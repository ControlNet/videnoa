# Videnoa Controller Task 20 Knowledge

## Reusable harness result

- The existing mock Videnoa already provides sufficient real-wire faults and generation-bearing checkpoints. Task 20 only needed a Controller-side real-TCP fixture that composes `controller_app_router`, real SQLite, rooted filesystem capabilities, authenticated bearer requests, and persistent mock worker registration.
- A full-pipeline test should pause the mock at `AfterRunPersistedBeforeResponse`, complete the deterministic first remote job while the response is held, release it, then pause/release `AfterDelete`. This gives event-driven completion without sleeps and leaves request counts, retained attempts, output bytes/hash, temp cleanup, and remote cleanup observable.
- Three one-slot workers can use the same pattern concurrently. Waiting on one run checkpoint per worker proves actual capacity distribution rather than merely checking scheduler candidates.

## Composition-root invariant

- Stage modules, lifecycle CAS operations, and recovery classifiers do not constitute a running pipeline. The production composition root must own a long-lived runtime that connects durable task creation and change notifications to reservation and every later stage.
- Startup recovery is incomplete if `Submit` and `Poll` commands are classified but discarded. Those commands must enter the same durable orchestration runtime used for normal work.
- Real HTTP intake can succeed with a durable queued row while every stage-level integration test remains green. Task 20 therefore requires at least one composition-root test that observes the first remote checkpoint, not only direct calls into `Scheduler`, `LifecycleService`, or `TransferExecutor`.

## Evidence qualification

- Prior Task 10-13 tests deterministically cover or model all irreversible crash boundaries, outages, no-clobber, retry, ambiguity, cancellation, and capacity invariants at stage scope.
- Until the orchestration runtime exists, those tests are valid component evidence but cannot be represented as an executed one-worker or multi-worker Controller process pipeline.

## Runtime implementation result

- `Orchestrator` subscribes to a bounded `EventHub` wakeup channel before its initial durable scan. Task intake and every durable lifecycle/worker/settings change trigger a rescan; a bounded poll tick covers remote processing, retry deadlines, transfer-permit release, and lagged notifications.
- Per-task stages run concurrently, but each task has at most one active stage. Immediate upload/download/publication/cleanup transitions loop through `Reconciler::reconcile_task_id`; processing and transient failures receive a per-task poll cooldown instead of tight polling.
- Remote job IDs are worker-local. Durable uniqueness must therefore be `(worker_id, remote_job_id)`, not `remote_job_id` globally; three independent workers can all legitimately return the same UUID-shaped first job ID.
- Multi-worker proofs must map each task through its durable `worker_id`. Zipping task creation order with fixture worker order is invalid because scheduler tie-breaking uses persisted worker IDs.
- Shutdown drain must count `StagePermit` lifetime as well as `WritePermit` lifetime. Closing intake, cancelling runtime admission, and waiting for both counters prevents a false drained result while a remote stage remains active.

## Executed restart matrix

- A recreated Controller generation must reopen the same SQLite path into a fresh `Store`. Reusing a cloned `Store` preserves its `OnceLock<ChangeObserver>` and prevents the new `EventHub` from becoming the durable-change observer.
- Abortive process simulation must await both aborted HTTP and orchestration join handles before reopening SQLite. The fixture retains only the temp directory, path configuration, authentication configuration, and optional one-shot checkpoint observer across generations.
- Irreversible local boundaries need observer points immediately before their durable CAS: verified download installed, destination staging creation/copy, final rename, local cleanup, and remote delete success. These points are production-neutral because the default observer remains a no-op.
- Lost submit responses may legitimately produce multiple `/api/run` requests while still producing exactly one remote job. Crash assertions must distinguish request replay from duplicate compute by checking retained attempt count, submission identity, and persistent mock job count.

## Distinct boundary proof

- A checkpoint in the remote handler after request arrival cannot prove pre-submit admission. The Controller-side observer must stop immediately before `VidenoaClient::run`, while route counters still show zero `/api/run` requests.
- Durable upload completion is the committed `Staged` task/attempt state after `FinishUpload`; it is distinct from `Submitting`, even when orchestration advances those states in one task loop.
- Remote terminal observation is only durable after `FinishProcessing` commits `RemoteCompleted`. Stop there before `StartDownload` so the download route count remains zero.
- A mid-body gate must yield at least one response byte first and pause only when the client polls for the next chunk; the Controller's `.part` length then proves positive transfer progress.

## Runtime rejection remediation

- Durable pause must be read at the irreversible boundary, immediately before keyed `VidenoaClient::run`; checking only reservation or upload admission leaves a staged task able to submit after pause.
- Runtime exits share one shutdown path: cancel HTTP intake first, persist pause, stop stage admission, drain stage/write permits, then await the surviving server or orchestration future while preserving the primary error.
- Stage outcomes require an explicit retryability partition. Remote outages and conflicts defer; persistence, invalid configuration, local path/I/O, lifecycle invariant, and malformed evidence failures terminate orchestration.
- Upload crash evidence is valid only when the mock has consumed a positive strict subset of the request body. Local-cleanup evidence needs a separate checkpoint after directory deletion and sync but before remote deletion.
- Strong convergence proof checks slot usage before and after completion, remote file count, exact cleanup requests, repeated submit idempotency keys, accepted cancellation, and distinct explicit-retry attempt identities.
- A pause check immediately before submit is still racy unless pause persistence and remote admission share an ordering primitive. A shared `RwLock` preserves concurrent worker submissions through read guards while making pause/settings updates exclusive through a write guard.
- Retryability belongs on the typed remote error and must match every variant exhaustively; stage orchestration and health recovery should consume that single classification rather than broad `Remote(_)` branches.
- A bounded drain is not successful merely because durable pause persisted. Timeout must cross the process boundary as a typed shutdown error carrying both outstanding stage and write counts.
- Evidence journals can retain route, request-shape, checkpoint, and response fidelity while replacing UUID text and redacting idempotency-key values only in the persisted copy; in-memory journals remain exact for replay assertions.

## Task-local remote failure isolation

- Remote failures need two independent classifications: retryability and ownership. Transient transport/server failures remain Orchestrator cooldown inputs; non-transient response failures owned by one submission or poll must be committed as that task's terminal lifecycle outcome instead of becoming process-fatal.
- A definite keyed submission rejection such as HTTP 400 maps to non-retryable `RemoteSubmissionFailed`. Conflict, malformed, oversized, or unexpected submission responses remain `RemoteStateAmbiguous` because remote acceptance cannot be disproved safely.
- Poll response failures that prevent trustworthy remote-state reconstruction map to non-retryable processing `RemoteStateAmbiguous`. They must not trigger automatic compute replay.
- Endpoint construction, invalid local paths, local I/O, client configuration, lifecycle CAS, persistence, scheduler, and worker-registry failures remain process-fatal because they are not isolated remote task outcomes.
- The durable failure write must hold the stage write permit. If that SQLite lifecycle commit fails, the error still propagates and terminates orchestration; task isolation must never swallow infrastructure failure.
- Real-TCP isolation proof requires two occupied workers: fault one task at submission or polling, assert its persisted failure code/stage, and independently drive the other task through output publication and cleanup on the same live Orchestrator.
- Mock completion helpers must wait for remote job persistence after releasing a pre-persistence checkpoint. Immediate completion otherwise races the handler and creates a false `JobNotFound` failure in concurrent suites.

## Evidence precision and cancellation cleanup

- Count plan boundaries from the acceptance wording, not from observer variants. Task 20 has nine required remote boundaries and six required local boundaries; observer points used to prove intermediate recovery state must be labeled auxiliary.
- A local crash checkpoint is useful evidence only when it asserts the pre-crash contract: durable task/attempt status, persisted publication evidence when applicable, `.part` absence, verified/evidence retention, staging/final visibility, temp workspace state, remote workspace state, and exact compute-request stability across restart.
- Cleanup retry evidence must snapshot upload, run, and download route counters before the failing delete and compare the exact values after convergence. Attempt count or remote job count alone does not prove transfer routes were not replayed.
- Terminal cancellation must not commit before task-owned remote workspace deletion succeeds or is already absent. Otherwise capacity can be released and the task marked cancelled while uploaded inputs remain orphaned permanently.
- A definite remote cancellation-cleanup rejection is task-owned and should persist a non-retryable cleanup failure through recovery-safe lifecycle failure handling; it must not become a process-fatal remote error merely because cancellation was already requested.
- Lost response recovery can resend an idempotent `/api/run` request without replaying compute. Evidence should state exact request counts and separately prove one persistent remote job rather than treating request replay and compute replay as identical.

## Downstream cancellation local cleanup

- Cancellation from `Downloading` or `Verifying` owns Controller-local task artifacts just as normal `RemoteCleanup` does. Terminal `Cancelled` is valid only after the entire `temp_root/<task-id>` workspace is absent, the temp-root directory entry is durably synced, and the remote workspace is deleted or already absent.
- Keep local workspace deletion in one shared idempotent helper. `NotFound` is success; successful removal requires parent-directory `sync_all`; other I/O failures must stop cancellation convergence before remote deletion.
- A Controller-local cleanup failure is recoverable infrastructure state, not a task-terminal failure. Preserve `cancel_requested_at` and the current nonterminal task/attempt status, return a typed retryable recovery error, and let bounded orchestration reconciliation retry.
- Real-path regressions should prove ownership with actual artifacts: positive `.part` bytes for `Downloading`, and both `.verified` plus `.verified.evidence` for `Verifying`.

## Complete durable scan invariant

- Recurring orchestration cannot use a finite one-shot prefix of nonterminal tasks: an old active, deferred, or unhealthy row at the front can permanently starve later eligible work.
- Use keyset pages ordered by `(updated_at_ms, id)` with a scan-start high-water tuple. The strict lower cursor prevents duplicate pages, the inclusive high-water bound makes each pass finite, and newly advanced rows beyond the bound are intentionally handled by the next wakeup or poll pass.
- Keep active ownership and eligibility checks after durable enumeration. Pagination changes which rows are visited, not whether already active compute may be repeated or whether retry, health, pause, and shutdown policy can be bypassed.
