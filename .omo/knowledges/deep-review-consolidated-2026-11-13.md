# Consolidated deep-review inventory (2026-11-13)

Scope: three review passes of application code at `fe2455a`, with only review
notes added in `66bae59` and `7bcc1a5`. This is the full inventory from these
passes, plus five historical issues rechecked in current code. It is not an
inventory of every old finding in the knowledge directory; already fixed issues
are excluded. All issues below remain unresolved in this review. No source fixes
were made.

Evidence: **runtime** means a bounded synthetic reproduction; **source** means
the cited code establishes the path but its fault/concurrency scenario was not
run; **design** marks a behavior whose intended policy must be clarified. P1 is
high priority, P2 normal priority, and P3 an edge-case correctness issue.

## Findings from these three passes

| ID | Priority / evidence | Issue and consequence | Primary location | Suggested direction |
| --- | --- | --- | --- | --- |
| 1 | P1 / runtime + source | Worker publishes terminal state in memory even if persistence fails. API reports completed while SQLite remains running; restart changes that durable row to cancelled. The API/DB mismatch was reproduced, restart consequence traced. | `crates/core/src/server/mod.rs:3441-3483`; `server/persistence.rs:203-220` | Make durable completion precede public completion; retain/retry persistence failures. |
| 2 | P2 / runtime | Multiple edges to a scalar input pass validation. Input-map insertion silently overwrites an earlier value; reversing edge order changes execution results. The editor permits creating this graph. | `crates/core/src/graph.rs:75-94,132-218`; `executor.rs:164-182`; `web/src/pages/editor/EditorPage.tsx:216-280` | Enforce one incoming edge per scalar input in backend and editor. |
| 3 | P2 / runtime | Outer validation does not validate the loaded inner graph's node types, ports, required inputs or DAG. A missing inner node type is admitted and fails at execution. | `crates/core/src/nodes/workflow_io.rs:384-403` | Recursively validate with shared cycle context and verify interfaces. |
| 4 | P2 / source | Active-job capacity check and admission are separate. Concurrent submissions or batches can exceed the 64-job limit. | `crates/core/src/server/mod.rs:413-424,1773-1802,2012-2036` | Reserve capacity atomically, with batch rollback. |
| 5 | P2 / source | Saving over an existing workflow directly truncates it. Interrupted or failed writes can destroy the previous graph; concurrent readers can see partial JSON. | `crates/core/src/server/mod.rs:2450-2452` | Same-directory temporary write and atomic replacement; version preconditions for concurrent editors. |
| 6 | P2 / source | Restart cleanup reconstructs only a subset of staged-output paths. Relative, computed, or input-default paths can leave temporary encodes; terminalization prevents later cleanup retry. | `crates/core/src/server/staged_outputs.rs:48-97` | Persist actual owned staging paths and durable cleanup obligations. |
| 7 | P1 / runtime + source | HttpRequest error context embeds raw request bodies, and the text redactor misses quoted JSON keys. A synthetic password marker survived a timed-out request; job/log propagation was traced. | `crates/core/src/nodes/http_request.rs:196-203`; `crates/core/src/logging.rs:477-500` | Omit raw bodies; use structured redaction when diagnostic fields are necessary. |
| 8 | P1 / bounded runtime | StringTemplate accepts effectively unbounded num_input and allocates that many ports during validation. A small graph can exhaust server resources before admission. Only 10,000 inputs were tested, producing 10,003 ports; OOM was not triggered. | `crates/core/src/nodes/string_template.rs:17-24,55-86`; `crates/core/src/graph.rs:325` | Bound input counts and total validation expansion before allocation. |
| 9 | P2 / runtime | HttpRequest retries ambiguous POST delivery, even after cancellation. A local server observed 3 POSTs from one execution with default retries, also when cancellation followed the first POST. | `crates/core/src/nodes/http_request.rs:205-220` | Require an idempotency contract for such retries and check cancellation between attempts/backoff. |
| 10 | P2 / design + runtime | Persisted outer jobs retain live nested-workflow paths. Editing the inner file changes the result of an unchanged outer snapshot; history does not identify all executed content. No explicit immutability promise was established. | `crates/core/src/nodes/workflow_io.rs:292` | Snapshot dependencies or pin identities; otherwise document live references and record resolved content. |
| 11 | P3 / runtime | Resize preview casts dimensions to u32 unchecked: 4,294,967,297 becomes 1. Full compilation rejects that same dimension. Normal resolutions are unaffected. | `crates/core/src/nodes/resize.rs:98-112`; `server/preview.rs:125`; `nodes/compile_context.rs:1221-1227` | Share checked dimension parsing and allocation limits. |
| 12 | P2 / runtime | Latest-only task update store loses an earlier distinct-task delta when updates are batched. The first row stays at version 1 while the second reaches version 2, with no corrective fetch. Browser scheduling was not tested. | `controller-web/src/events/taskUpdates.ts:15-21`; `controller-web/src/tasks/useTasksData.ts:130-161` | Retain per-task updates, buffer events, or refetch on generation gaps. |
| 13 | P2 / runtime | Biased SSE selection prioritizes queued events over overdue auth checks. A revoked session received 3 scheduler deltas after the 30-second deadline, then closed after the queue drained. | `crates/controller/src/operations/events.rs:118-143` | Prioritize overdue authentication or check the deadline before each emission. |

## Historical issues still present

These are source rechecks, not new fault reproductions. Items 17 and 18 were
previously left unfixed at the user's request; no remediation is performed here.

| ID | Priority | Issue and consequence | Location / direction |
| --- | --- | --- | --- |
| 14 | P2 | FFmpeg synchronous pipe IO and waits can outlive cancellation indefinitely if the subprocess stalls. Queued work can remain blocked behind that job. | `crates/core/src/nodes/video_input.rs:672,714`; `nodes/video_output.rs:764`. Add cancellation-aware subprocess supervision and bounded termination. |
| 15 | P2 | Dynamic TensorRT first-inference engine creation occurs after the cache lock is released; first-run cache-load failures also bypass static-shape recovery. Cross-process build coordination and broken-cache recovery are incomplete for dynamic models. | `crates/core/src/nodes/backend.rs:518-549`. Cover the actual dynamic build/load lifetime and first-run recovery. No GPU reproduction this review. |
| 16 | P2 | Non-square source SAR is lost and output forces square pixels. Anamorphic sources may have incorrect display proportions. | `crates/core/src/nodes/video_output.rs:374`. Propagate SAR or normalize pixel geometry while preserving display aspect. |
| 17 | P1 | Updating a worker endpoint redirects existing attempts because recovery/download resolve the current worker record. Jobs on the old machine can become unrecoverable or be read from the wrong machine. | `crates/controller/src/persistence/worker.rs:73-110`; `recovery/reconciler.rs:137-152`; `scheduler/download.rs:75-85`. Freeze endpoint identity for dependent attempts or reject edits while dependencies remain. |
| 18 | P2 | Concurrent login attempts can all pass the failure limiter before verification records failures, then queue on the password semaphore. The sequential five-failure rule does not bound concurrent admission. | `crates/controller/src/auth/service.rs:164-182`; `auth/password.rs:73-85`. Reserve attempt budget and bound verification waiters. |

Historical references: `codex-project-review-2026-10-23.md`,
`codex-controller-review-2026-10-23.md`, and
`engine-review-fixes-2026-10-30.md`.

## Additional observations, not counted as new application defects

- Test timing sensitivity: the initial controller full-suite command failed in
  `upload_response_headers_stall_after_body_eof` with
  `CheckpointTimeout("after_upload_bytes_accepted")`. Its isolated rerun and
  complete serial client-test rerun passed. The test uses a 100 ms stall deadline
  before reaching the intended checkpoint. Timing sensitivity is plausible;
  the exact failure cause was not traced. The original full run is not a pass.
- README-controller.md still describes changed source identity as fail-closed
  without clearly delimiting intake. Current-content uploads are intentionally
  authorized in `controller-current-input-upload-2026-09-07.md`; clarify prose
  rather than treating that upload policy as a new defect.

## Verification and reproducibility

- First pass: core library 881 passed / 24 ignored; worker frontend 299 tests;
  controller frontend 196 tests; synthetic probes 3 passed.
- Initial controller full command exited 101 in its client binary (37 passed,
  1 failed). Earlier task20 recovery suite passed all 71 tests. Isolated failed
  test rerun passed; full client binary serial rerun passed all 38 tests.
- Second pass: 4 synthetic probes and 9 existing HTTP-node tests passed.
- Third pass: 1 SSE probe passed; UI probe plus existing task-hook tests passed
  all 4 tests. All 6 existing SSE regression tests passed (48.22 seconds).

Full probe sources and detailed evidence are retained in:

- `deep-review-2026-11-13.md` (IDs 1-6).
- `deep-review-second-pass-2026-11-13.md` (IDs 7-11).
- `deep-review-third-pass-2026-11-13.md` (IDs 12-13 and historical rechecks).

The probes use synthetic data, temporary storage and loopback services, never
production credentials. They demonstrate existing defective behavior and must
be inverted/adapted when used as regression tests for fixes. Temporary test files
are removed from the working tree. No application source changed. No GPU run,
real browser E2E, production service request, real power loss, disk-full fault,
or deliberate OOM was tested. Source-based concurrency findings are explicitly
separated from bounded runtime evidence above.
