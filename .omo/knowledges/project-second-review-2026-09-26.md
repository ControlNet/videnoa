# Second project review, 2026-09-26

Reviewed baseline: `8b84dfd` on `dev`. This is an assessment, not a fix session.
Application source remains unchanged. The previously deferred Downloader
basename collision is still open and is not counted again below.

The review traced Worker decode/encode completion, scalar cancellation and
persistence, preview extraction/serving/processing, workspace transfers,
Controller remote submission/polling/upload/download/publication/cleanup, and
the two frontends' event consumers and submission flows. This does not claim
exhaustive coverage of every platform, dependency or GPU backend.

## 1. P1: decoder failure is accepted as successful end of video

Location: `crates/core/src/nodes/video_input.rs:538-546`, consumed by
`crates/core/src/streaming_executor.rs:499-535`.

`VideoDecoder::next` converts EOF to None without calling `finish`, which is
where the child exit code would be checked. Partial raw frames are also discarded
as ordinary EOF. Streaming execution only sees the iterator, so a fatal decoder
exit can become successful pipeline completion. Encoder verification checks the
result's existence/streams/dimensions but not completeness against the source.

An isolated fault-injection executable delegated decoding to actual system
FFmpeg, emitted one real frame, then deliberately exited with status 7. Encoding
still used unmodified system FFmpeg. A real API job with a generated 10-frame
32x32 input finished as `completed`; ffprobe counted exactly one output frame.
This demonstrates a valid but truncated output being reported as successful.
Controller trusts the Worker completion and byte-transfer evidence, so its
downstream verification does not independently catch this media truncation.

Fix direction: propagate decoder exit and partial-frame errors through the
iterator, distinguish cancellation from clean EOF, and test incomplete outputs.

## 2. P1: preview filename escapes the session directory

Location: `crates/core/src/server/mod.rs:2357-2376`.

The route percent-decodes its filename parameter and directly joins it to the
session directory. It does not require a single filename or verify containment.
An encoded absolute path replaces the session directory during PathBuf::join.

After creating a legitimate preview, a request whose filename was an encoded
absolute path to a separate temporary text fixture returned HTTP 200 and the
fixture's exact bytes, labelled `image/png`. No production or private file was
read. This is a read escape by a caller able to access the preview API and obtain
a session ID; it does not bypass configured authentication. With authentication
disabled, the same route is available to anonymous callers.

Fix direction: accept only the generated frame/processed filename forms and
enforce directory containment and safe file opening.

## 3. P2: failed job deletion restores an irreversibly cancelled queued job

Location: `crates/core/src/server/mod.rs:1711-1744`.

Deletion removes the job and cancels its token before attempting persistent
deletion. On SQLite failure it reinserts the original snapshot and sender, but
the token remains cancelled and the queued executor exits. The API still reports
the restored job as queued; there is no automatic replacement executor.

The probe held one actual local HTTP job to occupy admission, then queued a
second job. A synthetic SQLite BEFORE DELETE trigger failed deletion in the
isolated test database. DELETE returned 500. After removing the trigger and
releasing the first request, a third job completed while the restored second
job stayed queued. Two isolated executions confirmed this behavior. A combined
probe run also hit a five-second HTTP fixture-start timeout before testing this
condition; that harness failure is not treated as a production finding.

Fix direction: coordinate persistence and cancellation as a coherent transition;
do not restore an executable queued/running state with a cancelled token.

## 4. P2: preview extraction blocks Tokio runtime workers

Location: `crates/core/src/server/mod.rs:2283-2324`.

The async handler directly uses blocking process `.output()` for both ffprobe
and FFmpeg. The probe also uses `-count_frames`, scanning video frames before
extraction. Unlike preview processing, extraction has no blocking-task boundary
or admission limit. Concurrent long extractions can occupy all runtime workers
and delay unrelated requests and cancellation handling.

A synthetic ffprobe wrapper delayed for one second before delegating to real
ffprobe. Running extraction on a current-thread runtime delayed an unrelated
20 ms async timer to approximately 1.14 seconds. This confirms blocking of the
executor; saturation of an arbitrary production thread count was not benchmarked.

Fix direction: move subprocess work off async workers, bound extraction
concurrency, and define subprocess timeout/cancellation behavior.

## 5. P2: successful submission becomes an apparent failure after list refresh

Location: `web/src/stores/job-store.ts:56-71`, displayed by
`web/src/pages/editor/RunFromEditorDialog.tsx:87-98` and other store consumers.

The store awaits job-list refresh before returning an already-created job ID.
A failed follow-up GET rejects the whole submission promise, and the UI displays
its submission-failure message. A user retry creates another job. The same
conflation exists in saved-workflow submission and rerun operations.

A synthetic transport accepted POST with HTTP 201 and a job ID, then rejected
only the following list GET. The store rejected with the list error and retained
no job in its local list. Retrying the same action created a second job.

Fix direction: preserve/return the confirmed creation receipt independently of
refresh, and surface refresh failures without inviting a duplicate submission.

## 6. P2: global task counters ignore transitions outside the selected filter

Location: `controller-web/src/tasks/useTasksData.ts:98-104`.

When an incoming task is absent from the loaded page and does not match the
current query, the hook returns without refreshing. Its global status counters
are only loaded alongside the task page, so they remain stale too. This is
independent of the selected-detail recovery fix.

The probe loaded a processing-only page with global counts showing one queued
task and one processing task. It then changed the synthetic server's queued task
to failed and published the corresponding task update. The hook made no second
counts request and still showed queued=1, failed=0. A later unrelated refetch
would repair it, but the event itself does not.

Fix direction: invalidate/update global counts independently of page membership,
with coalescing so per-frame progress updates do not create request storms.

## 7. P2: preview sessions and successful images have no reclamation path

Location: `crates/core/src/server/mod.rs:2278-2281`, `2344-2347`, `2409-2426`.

Every extraction creates a new system-temporary directory and inserts it into
an unbounded session map. Closing the dialog or selecting another video does
not release it. There is no session deletion endpoint, TTL, startup sweep or
application-state destructor removing these directories. Each successful
processing request now adds a unique PNG; only failed processing removes its
own partial output. Failed extraction can also leave its directory behind.

The path-read probe additionally verified the session directory still existed
after the owning application/router was dropped, then explicitly removed its
own fixture directory. Static inspection found no production removal path.
Repeated use therefore accumulates images on disk; memory entries accumulate
while the service runs. External OS temporary-file cleanup is environment-specific
and is not an application lifecycle policy.

Fix direction: implement bounded session lifetime/size, explicit release and
orphan cleanup that respects active preview requests.

## Evidence and verification

All fault injectors, media and HTTP responses were explicitly synthetic test
fixtures. Application implementations were never replaced or patched. Temporary
probe files were moved out of the repository after execution:

- `/tmp/videnoa-second-review-probes-20260926/second_review_probe.rs`
- `/tmp/videnoa-second-review-probes-20260926/controller-second-review-probe.test.tsx`
- `/tmp/videnoa-second-review-probes-20260926/worker-second-review-probe.test.ts`

Core probes confirmed decoder truncation, preview path escape, extraction
blocking and failed-delete stranding. The first three passed together; the
delete probe passed in isolated runs. Frontend probes confirmed counters and
submission ambiguity. The Controller fixture also demonstrated lost updates
when two store publications are forced into one React act batch. That candidate
is excluded from findings: native EventSource delivery/render scheduling was not
reproduced and may prevent that synthetic batching scenario.

Logs: `/tmp/videnoa-second-review-core.log`,
`/tmp/videnoa-second-review-delete-verified.log`,
`/tmp/videnoa-second-review-ui-final.log`,
`/tmp/videnoa-second-review-submit.log`. The combined harness timeout is recorded
in `/tmp/videnoa-second-review-core-final.log`.

No Windows runtime, physical sleep/resume, release packaging, TensorRT engine
build, dependency vulnerability audit or native browser end-to-end run was
performed in this review. The previous fix session's GPU proof is not counted
as a new GPU test here.

Existing suites rerun after removing the temporary probes all exited zero:

- Core library and server regressions: 632 passed, 11 ignored.
- Controller lifecycle, task13 and task_api: 113 passed.
- Worker frontend: 197 passed; Controller frontend: 182 passed.

Exact commands, from the repository root:

```bash
cargo test --locked -p videnoa-core --lib --test server_regressions
cargo test --locked -p videnoa-controller --test lifecycle --test task13 --test task_api -- --test-threads=4
npm --prefix web test
npm --prefix controller-web test
```

These ordinary tests should pass but do not assert the failure cases established
by the probes. The full Controller all-target suite, builds and linters from the
previous fix session were not repeated because application source is unchanged.
