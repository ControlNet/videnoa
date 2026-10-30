# Design review (2026-10-30, dev at c41c91a)

## How the review ran

- Five read-only Claude subagents each covered one design concern:
  - the worker execution engine;
  - the worker service layer (API, persistence, config, desktop);
  - the controller core;
  - the controller-worker contract across versions;
  - both frontends.
- Each one got the list of known, deferred and already fixed findings, so
  they would not report them again.
- Probes ran only in scratch copies of the workspace. The repo was not
  modified.
- Claude re-read the code for the high findings: A, B, C, D, E, I, and the
  Docker part of R.
- Findings reported by two reviewers independently are merged. The note
  after each title says how it was confirmed.

## Findings, ranked

### High

**A. A worker-level error fails tasks permanently and orphans the remote job.**
Confirmed by running, independently by two reviewers.

- Where:
  - `recovery/remote_failure.rs:95-110`: a poll that ends in `NotFound`,
    `RateLimited` or `ClientStatus` becomes `remote_state_ambiguous`;
  - `lifecycle/classification.rs:62`: retry is `Blocked`;
  - a `Failed` task cannot be cancelled;
  - nothing calls `cancel_job` on this path.
- Triggers:
  - the worker password is set or changed (401);
  - the worker's login limiter answers 429;
  - a proxy answers 404/3xx;
  - the worker returns a new `JobStatus` value (closed enum, giving
    `MalformedPayload`).
- Result:
  - every Processing task on that worker fails at once, and cannot be
    retried or cancelled;
  - the GPU job keeps running, and its output is never downloaded;
  - the freed slot submits the next task behind the orphaned job.
- The same misreading occurs in two more places:
  - the cancel-during-submit path (`recovery/submission.rs:180`, where
    401/403 is taken as `NotAccepted`);
  - remote cleanup (`scheduler/cleanup_remote.rs:65-81`).
- The health probe already treats 401/403 as a worker problem
  (`workers/health/probe.rs:103`).
- Fix direction: give remote errors a scope. Worker-level errors pause or
  defer that worker's tasks. Only job-specific evidence fails a task.
  Terminal ambiguity should make a best-effort `DELETE /api/jobs/{id}`.

**B. A non-transient health error during reconcile stops the controller.**
Confirmed by running, independently by two reviewers.

- Where:
  - `recovery/reconciler.rs:147-154` returns `Err(error) => return
    Err(error.into())`;
  - `recovery_scan.rs:54` then exits the run loop;
  - `main.rs` uses `try_join!`, so the HTTP server goes down too.
- Triggers: `/api/health` returns 404/403/302/HTML, or a new health value
  (`HealthStatus` is closed). An iroh worker with a stale password also
  triggers it, because the tunnel 401 becomes `ClientStatus 401`.
- Crash loop:
  - after a restart the health backoff expires;
  - `worker_health_deferred` stops deferring;
  - the first pass exits again.
  - The UI where the password would be fixed is down too.
- Probes:
  - `{"status":"degraded"}` → `Recovery(Remote(MalformedPayload))`;
  - 404 → `Recovery(Remote(NotFound))`.
- The same pattern exists for `cancel_job` errors at
  `recovery/submission.rs:128-130` and `:211-213`.
- It contradicts the task-20 rule that remote failures must not be
  process-fatal.
- Fix direction: gate on the durable worker health record rather than an
  inline probe, and treat every remote reply as deferral or auth-block,
  never as fatal.

### Medium-High

**C. A saved workflow with a preset's name silently replaces that preset in
`/api/run`.** Confirmed by running.

- Where:
  - `server/mod.rs:1547-1575` searches `workflows_dir` first, then
    `presets_dir`;
  - controller `run_name()` (`domain/values.rs:43`) strips `.json`;
  - the controller's identity check compares only `workflow_name`, `input`
    and `output`.
- Scenario: a user saves a workflow named `interpolation-2x`. From then on,
  controller tasks for that preset run the user's graph. The output is wrong
  and nothing reports it.
- Variants:
  - `/api/presets` is a snapshot taken at startup, while `/api/run` reads
    live from disk;
  - presets created through the API get UUID ids and live only in memory,
    so they are listed but give 404 on run. The upload still happens
    before the terminal failure.
- Fix direction: give presets and workflows separate namespaces in the run
  request, or reject a save that collides with a preset name.

**D. The editor's Save can overwrite a different workflow.** (a) confirmed by
running; the others confirmed by reading.

- (a) Load A, load B, Ctrl+Z, Save. The canvas shows A, but `currentFile`
  is still B, so B.json gets A's graph. History entries hold only nodes and
  edges (`workflow-store.ts:31`).
- (b) Save writes by the JSON `name`, not by filename
  (`EditorToolbar.tsx:119`, `server/mod.rs:1991` and `:2048`). A file copied
  on disk therefore overwrites the original.
- (c) Save As replaces an existing file without asking (`std::fs::write`).
- (d) There is no dirty flag and no `beforeunload`, so loading, clearing or
  closing drops unsaved work.

### Medium

**E. Cancel is "delete history", and the job keeps the only GPU permit while
invisible.** Confirmed by reading, by two reviewers.

- Where:
  - `server/mod.rs:1797-1812` removes the entry, then cancels;
  - nothing sets `JobStatus::Cancelled` at runtime; only startup restore
    does;
  - node initialisation, including the first TensorRT engine build
    (8-12 min), runs with `ExecutionContext::default()` and has no cancel
    check (`compile_context.rs:421`, `:431`).
- Scenario: a job is cancelled while its engine builds. DELETE returns 204
  and GET returns 404, but the permit is still held. The controller's next
  task sits `queued` with no visible cause.
- The streaming executor returns `Ok(())` when cancelled.
- This is separate from the deferred "stalled ffmpeg" finding: it happens
  even when everything cancels cooperatively.

**F. The controller-worker contract has no version channel and no shared
types.** Confirmed by reading and by git history.

- There is no version negotiation:
  - the controller never calls `/api/about`;
  - the worker never reads the controller's User-Agent.
- The DTOs are copied by hand three times:
  - worker server;
  - `controller/src/remote/dto.rs`;
  - the test mock.
  The mock copy has already hidden one real bug.
- Closed enums (`JobStatus`, `HealthStatus`) feed A and B.
- The worker's `RunWorkflowRequest` uses `deny_unknown_fields`. A new
  controller request field therefore gets 422 from an older worker, after
  the input upload has finished.
- Frontends:
  - the worker UI casts responses unchecked;
  - controller-web uses `.strict()` zod and closed enums, so one new field
    on Task rejects the whole task list;
  - `ApiErrorCode::RateLimited` is already missing in controller-web.
- Fix direction:
  - a small dependency-free `videnoa-protocol` crate shared by core, the
    controller and the mock;
  - health reports a protocol version;
  - `#[serde(other)] Unknown` on status enums;
  - tolerant list parsing;
  - TS types generated, or checked against fixtures.

**G. The saved workflow format has no version, and unknown params are
ignored.** Confirmed by running.

- Where:
  - `graph.rs:329` has no version field;
  - node factories ignore unknown param keys (`registry.rs:91-104`);
  - the editor hides nodes whose type is unknown (`CustomNode.tsx:629`) but
    still exports them;
  - non-primitive params are turned into strings (`null` becomes `"null"`).
- Real drift already exists: the descriptor exposes a `device_id` port on
  SuperResolution and FrameInterpolation, but the runtime ignores it.
  `backend.rs:256` hard-codes device 0, so setting `device_id = 1` silently
  uses GPU 0.
- A renamed or removed param in an old workflow runs with the default, and
  nothing reports it.

**H. Validation accepts graphs that cannot run.** Confirmed by running.

- A scalar node downstream of a frame-chain node fails after 201 and after
  taking the GPU permit, with `missing outputs for upstream node 'input'`.
  Example: `VideoInput.source_path → PathDivider → PathJoiner →
  VideoOutput.output_path`.
- A `Workflow` node pointing at a video workflow can never run, but
  `/api/run/validate` accepts it.
- The interface stored in the outer node's params is a second source of
  truth.

**I. The TensorRT engine cache key leaves out the TensorRT and ONNX Runtime
versions.** The key contents are confirmed by reading; the failure mode is
suspected (no GPU A/B run).

- The key is `device-0_{sha256}_{h}x{w}[_tile…]` (`backend.rs:353`).
- The Dockerfile moved from TensorRT 10.7 to 10.9, and README tells users to
  persist `trt_cache`. ONNX Runtime's documentation leaves cleaning old
  engines to the user.
- Writes are not atomic and are not locked across processes.
- Fix direction: put the versions in the key; on a deserialise error, move
  the entry aside and rebuild once; build into a temporary location and
  rename it into place.

**J. Tasks on a worker that is gone for good have no exit.** Confirmed by
running: still Processing with the cancel recorded after 20 s.

- Cancel can finish only after a successful remote delete.
- Retry requires `Failed`, which these tasks never reach.
- Deleting the worker is refused while any attempt references it.
  - Migration 0014's advice to "delete the Worker" can therefore never be
    followed.
- The only way out is editing the endpoint, which is the known deferred
  bug.
- Fix direction: an operator action to abandon a task or decommission a
  worker.

**K. Failed tasks never release disk.** Confirmed by reading.

- `fail_lifecycle` only updates rows.
- What is left behind:
  - local `data/temp/<task>/` files, including full-size verified outputs
    after `OutputExists`;
  - the worker-side `<task>/` folder;
  - history tables, which have no retention.

**L. One malformed preset interface takes the whole worker offline in the
controller.** Confirmed by reading.

- `/api/presets` is parsed as a single typed list (`dto.rs:51`), and the
  worker serves `workflow` without validating it.
- Fix direction: parse each entry separately and mark bad entries invalid.

**M. The worker's persisted state has no version, and a broken job store only
logs a warning.** Confirmed by reading.

- `jobs.db` and the auth DB use `CREATE TABLE IF NOT EXISTS` with column
  probing, and no `user_version`.
- `config.toml` has no version. A downgraded build drops unknown keys the
  next time it saves.
- If the job store fails to open, the worker continues in memory only
  (`server/mod.rs:144-154`), and health still says ok. The controller then
  sees 404s and marks tasks ambiguous.
- Rows that cannot be parsed are skipped silently.
- Rerun skips validation.

**N. One first-come-first-served GPU semaphore for all work, and capacity is
not advertised.** Confirmed by reading.

- Preview waits behind every queued job, with no timeout.
- Download-only and HTTP-only workflows hold the GPU permit.
- The queue has no bound.
- The controller's `compute_slots` is never checked against the worker's
  fixed limit of 1.

### Low-Medium and Low

- **O. DNS rebinding with open access.** Confirmed by running.
  - When no password is set, Host is never checked, and the same-origin
    check compares Origin with the request's own Host.
  - A rebinding page can therefore save workflows, browse the filesystem
    and set the first password.
- **P. A panic in `run_job` leaves the job `running` until restart.**
  Confirmed by reading; no trigger found.
  - The spawned handle is dropped, and there is no `catch_unwind` around
    the compile and scalar phases.
- **Q. Shutdown and data-dir ownership are undefined.**
  - There is no signal handling. In Docker, videnoa runs as PID 1 with no
    init, so SIGTERM is ignored and `docker stop` ends with SIGKILL.
  - Child processes have no process group or job object.
  - A second instance on the same data dir rewrites running rows to
    `cancelled` before it fails on the auth lock (confirmed by running with
    a simulated row).
- **R. Relative paths have several bases.**
  - `workflows_dir` resolves against the working directory, not the data
    dir. The standard Docker mount `/app/data` happens to cover it; a custom
    `VIDENOA_DATA_DIR` does not.
  - `model_path` in presets is relative to the working directory, so
    `paths.models_dir` affects only the listing.
  - The desktop app uses `data_dir(None)`, with no app-data anchor.
- **S. A deterministic worker 500 before acceptance is retried forever**
  while the task stays in Submitting. Examples: no job persistence, an
  unreadable workflow file. The controller never reads the worker's error
  `code`.
- **T. A password change in one worker UI tab breaks mutations in other tabs**
  with 403. Only 401 is handled. The controller UI already refetches the
  CSRF token on 403.
- **Settings are saved as a whole document.**
  - The desktop language switch and the Settings form can revert each
    other.
- **"Print output" is only broadcast live** at compile time, so it is lost
  for jobs subscribed late.
- **i18n:** some literals are hard-coded English, such as "Print output",
  "Select model" and "Endpoint ID". `auth.Failed to fetch` shows as a raw
  key. Locale parity is not enforced by a type or a test.
- **Silent failures remain:**
  - a failed preset list shows an empty gallery with no message;
  - the node-definition error is never displayed;
  - `listDirectory` and `browseDirectory` return `[]` on any error.
- **Cancelling a Submitting task probes with `/api/run`**, which can create
  the job it is cancelling.
- **The state vocabulary is copied in many places:** SQL literals for
  occupancy, CHECK constraints and the UI's action policy. Migration tests
  start only from an empty database.
- **Transport version skew, or a worker key reset, looks like an ordinary
  outage.** CONNECT 400 and ALPN errors are mapped to transient `Network`.

## Checked and found sound

- **Engine:**
  - stage channels are bounded;
  - the graph must be linear, so a fan-out branch cannot stall;
  - stage panics become job errors;
  - the frame pool has a single owner;
  - the GPU permit is released on unwind;
  - preview moves its permit into `spawn_blocking`.
- **Worker:**
  - restart turns running and queued jobs into `cancelled`;
  - idempotency claims are atomic;
  - the preview cache is bounded;
  - Argon2id with hashed session tokens;
  - iroh and HTTP share one router and one auth check;
  - logs rotate and are redacted.
- **Controller:**
  - WAL plus write-first transactions;
  - task and attempt are updated in one compare-and-swap;
  - crash recovery is idempotent at every step (keyed submit replay,
    stat-then-PUT upload, verified download, no-replace publish);
  - pause admission is ordered;
  - a task that cannot be placed does not block those behind it;
  - unbounded submit confirmation is a deliberate choice.
- **Contract:**
  - response DTOs ignore unknown fields since v0.1.9;
  - worker response structs are unchanged since v0.1.7;
  - the controller sends a workflow name, never a graph;
  - upload and download retries are bounded.
- **Frontends:**
  - the worker job list resyncs through its 2 s poll;
  - controller SSE resyncs after errors, lag and sleep;
  - controller settings are versioned;
  - locale key sets currently match.

## Probe locations (scratchpad, temporary)

All probes are in the session scratchpad and are not committed:

- `contract-repo/crates/controller/tests/task20/worker_health.rs`;
- `repo/crates/controller/tests/task20/design_review_probe.rs`;
- `engine-probe/ws/crates/core/tests/engine_probe.rs`;
- `fe/webcopy/src/review/design-review.test.tsx`;
- `worker-probe/`.
