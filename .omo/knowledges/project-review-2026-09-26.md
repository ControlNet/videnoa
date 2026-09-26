# Project review, 2026-09-26

Reviewed baseline: `67b40ab` on `dev`. This was a review, not a defect-fix session.
Application source was left unchanged. Temporary probes used synthetic HTTP
bodies, a generated one-frame video, and isolated data directories; they never
used production Controller data or real media as mutable fixtures.

## Confirmed findings

### P1: Downloader results collide by filename

`crates/core/src/nodes/downloader.rs:284-320` computes a URL hash but only uses it
when neither Content-Disposition nor the URL provides a filename. Normal
downloads share `temp_dir()/videnoa/downloads/<filename>` and the corresponding
`.part` path. The completed temporary file is renamed over the final path.

An actual local HTTP server served different bodies at `/a/<unique-name>.mkv`
and `/b/<unique-name>.mkv`. Sequential calls to the production DownloaderNode
returned identical paths, and the first result contained the second response
after the second call. Concurrent invocations also share their temporary path.
Two downloads in one workflow can therefore invalidate an upstream path before
its consumer reads it; separate CLI processes also share this directory.

Use execution-specific directories, or a URL namespace plus unique temporary
files and an explicit ownership/lifetime policy. Preserve the original basename
inside that namespace for PathDivider and naming workflows.

### P1: Cancelling a scalar workflow does not stop downstream nodes

`crates/core/src/server/mod.rs:1707-1720` cancels the token and removes the job
when DELETE is called. `crates/core/src/executor.rs:103-177` never checks
cancellation in its scalar node loop; the parameterized scalar executor also
has no cancellation receiver. The watch receiver is only consumed by the
streaming execution branch.

A real two-node HttpRequest workflow was paused inside the first local HTTP
handler. DELETE returned 204, then releasing the first handler still caused
the second HTTP request. GET of the deleted job returned 404. This is more than
failure to interrupt an already-running operation: new downstream side effects
continue after deletion, while the execution still owns the worker semaphore.

Propagate cancellation to both scalar execution paths and check it between
nodes. Long-running network nodes need cooperative cancellation as well.

### P2: Worker JSON parameter conversion loses declared Path types

`crates/core/src/server/mod.rs:2551-2565` infers PortData solely from JSON shape,
so all JSON strings become Str. WorkflowInput forwards supplied PortData without
coercing it to the declared port type. This also affects parameters inferred
from WorkflowInput node settings by POST `/api/jobs`.

An API-submitted `WorkflowInput(Path) -> PathDivider` graph failed with
`PathDivider requires input port 'path' of type Path`. The identical graph with
explicit empty top-level `params` completed, because node-local JSON was then
decoded using its declared port type. VideoFrames graphs take a different
parameter-injection branch and were not implicated by this probe.

Decode against declared WorkflowInput types, or consistently inject JSON into
the graph and let the existing typed decoder handle it.

### P2: Preview processing returns the original frame without inference

`crates/core/src/server/mod.rs:2379-2403` accepts but never consumes `workflow`,
then returns the original extracted frame URL. The UI at
`web/src/pages/preview/ComparisonViewer.tsx:523-533` displays it as the processed
result.

A generated 32x32 one-frame video was successfully extracted. A processing
request containing a nonexistent node type still returned HTTP 200 and exactly
the original frame URL. The current before/after view cannot demonstrate model
output. Implement processing and validation, or explicitly report unsupported
processing until it exists.

### P2: Controller detail ignores SSE recovery invalidation

`controller-web/src/tasks/useTaskDetail.ts:190-196` reloads on selected-task
deltas, but does not subscribe to `appInvalidationStore`. SessionEvents sends
initial/refetch/lag/reconnect notifications through that separate store. The
task list subscribes; the selected detail pane retains the same taskId and does
not remount when the list refreshes.

A hook regression probe loaded the detail, published a reconnect invalidation,
and forced a parent-style rerender. The expected second detail request never
occurred: expected request count 2, actual count 1. A terminal transition lost
during disconnection can therefore leave the drawer indefinitely stale even
after the list is current. Subscribe to recovery invalidation while retaining
the existing visible detail and expanded history during refresh.

## Verification

Final results:

- Worker/core/app/transport tests: 656 passed, 11 ignored.
- Controller all-target tests: 600 passed, 1 ignored, exit status 0. All 37
  task20 crash/outage tests passed (212 seconds); no PoolTimedOut was observed.
- An additional isolated run of the five task21 load/concurrency/filesystem/
  resource/security suites passed 140 tests. These overlap the all-target run.
- Worker web: 193 unit tests passed; lint and build passed.
- Controller web: 170 unit tests passed; lint and build passed.
- Workspace formatting and strict Controller Clippy passed.
- Four temporary Rust characterization probes confirmed findings 1-4.
- One temporary frontend regression probe failed as expected for finding 5.

Commands, run from the repository root:

```bash
cargo fmt --all -- --check
cargo test --locked -p videnoa-transport -p videnoa-core -p videnoa-app --lib --tests
cargo test --locked -p videnoa-controller --all-targets
cargo clippy --locked -p videnoa-controller --all-targets --all-features -- -D warnings
npm --prefix web test -- --run
npm --prefix web run lint
npm --prefix web run build
npm --prefix controller-web test -- --run
npm --prefix controller-web run lint
npm --prefix controller-web run build
```

Existing quality gates should exit zero; explicitly ignored GPU/public-network
tests are not evidence of runtime validation. Worker web emits a bundle-size
warning. No GPU inference, release packaging, Docker build, or browser E2E was
performed in this review.

Local review artifacts (temporary, not durable repository assets):

- `/tmp/videnoa_review_probe_executed.rs`: four Rust probes.
- `/tmp/videnoa_review_detail_probe.test.tsx`: detail hook regression probe.
- `/tmp/videnoa-review-probes.log`: confirmed runtime observations.
- `/tmp/videnoa-review-detail-probe.log`: missing-refetch assertion failure.
- `/tmp/videnoa-review-rust.log` and `/tmp/videnoa-review-controller-rust.log`:
  backend suite output.

Temporary probes were moved out of the repository after execution. They assert
observed defects or expected corrected behavior as described above and are not
replacement production implementations.
