# Fixes for the 2026-11-13 deep review

Source: the Codex review in `deep-review-consolidated-2026-11-13.md`. Claude
re-read the code for the 13 new findings before fixing them. Fixed here:
1-9, 11, 12, 13. Not fixed:

- **10** (nested workflows are read live by path). The owner decided on
  2026-11-15 to keep the live reference: a queued job or a rerun runs the
  nested file as it is when the node executes, not as it was at submission.
  This is intended, not a defect; `WorkflowNode::execute` in
  `crates/core/src/nodes/workflow_io.rs` says so. Submission still validates
  the nested graph (fix 3), so a nested file edited after submission is only
  checked when it runs.
- **14-16** are older open items.
- **17 and 18** the owner deferred earlier.

Each fix has a regression test that failed before the change.

| # | Commit | Change | Test |
|---|---|---|---|
| 2 | 72a4814 | `PipelineGraph::validate` rejects two or more non-VideoFrames connections into one input port. The editor's `addEdge` replaces the edge already on that target handle. VideoFrames fan-in keeps its own message from `validate_video_workflow`. | `graph::tests::two_connections_into_one_input_port_are_rejected`; web `workflow-store.test.ts` "addEdge" |
| 5 | 157053e | Overwriting a saved workflow writes `.<file>.<rand>.videnoa-save` beside it, syncs it and renames it over the old file (`replace_workflow_file`). The staging name has no `.json` suffix, so the list never shows it. | `test_save_workflow_overwrite_replaces_the_file_instead_of_truncating_it` (unix: a hard link to the old file keeps the old content) |
| 7, 9 | b68c2cf, 98da4b0 | HttpRequest errors report `body=<N bytes>`, not the body. `redact_sensitive_assignments` also finds quoted keys (`"password" : "x"`, `'token':'y'`). A method that is not idempotent (anything except GET, HEAD, OPTIONS, TRACE, PUT, DELETE) is retried only after a connect error. Each attempt and the backoff, slept in 50 ms slices, check `ctx.check_cancelled()`. | `test_http_request_error_omits_the_request_body`, `..._does_not_resend_a_post_that_may_have_arrived` (POST 1 connection, GET 3), `..._stops_retrying_once_cancelled`, `logging::tests::redact_sensitive_text_masks_quoted_json_keys` |
| 8, 11 | 94d98fb | `StringTemplateNode::MAX_NUM_INPUT = 64`. `from_params` now returns `Result` and refuses a larger count, and so does `execute`. The editor shows at most `MAX_STRING_TEMPLATE_INPUTS` (64, `web/src/types/index.ts`). Resize uses `u32::try_from` for width and height, as compile already did. | `test_num_input_above_the_limit_is_rejected`, `test_resize_execute_rejects_a_width_beyond_u32` |
| 3 | f33ef24 | `validate` is split into `validate_own_nodes` plus the nested walk. `check_nested_workflow` runs `validate_own_nodes` on each nested graph along one `chain`. Calling plain `validate` there would restart the chain at each level and recurse without bound on a self-nesting workflow. | `validate_nested_workflow_validates_the_inner_graph` (one and two levels deep) |
| 1 | 9e26f53 | When `finish_job` fails to persist a terminal state, it adds the job id to `unpersisted_terminal`. A task retries those after 0.25, 1, 4, 15, 60 and 300 s. `wait_for_jobs_to_stop` writes them once more at shutdown. The API still shows the terminal state at once. | `test_a_failed_terminal_write_is_retried`, `test_shutdown_writes_a_terminal_state_that_failed_to_persist` (SQLite trigger fault injection) |
| 4 | 4630679 | `admit_jobs(n)` locks `AppStateInner::admission`, checks capacity and returns the guard. Single, idempotent and batch submissions hold the guard until `spawn_job` has put the jobs in `jobs`. No `.await` runs while it is held. | `concurrent_submissions_do_not_overfill_the_queue`: 16 concurrent POSTs for one free slot admitted 4 before the fix |
| 6 | 8b383db | The startup sweep joins a relative `output_path` to the current working directory, which is where the job opened it. It also uses the `default_value` of the wired `WorkflowInput` port, after the job param and the node param. Paths computed by other nodes are still unknown to it. Persisting the real staged path would need a schema change. | `output_paths_cover_relative_paths_and_input_defaults` (unix) |
| 12 | 8ee1325 | The task and worker update stores keep the last 256 deltas (`events/deltas.ts`). `useTasksData`, `useTaskDetail`, `useWorkersData` and `useWorkerNames` apply all of them, and refetch if some were already dropped. | controller-web `useTasksData.test.tsx` "applies every task update that arrives in one batch", `deltas.test.ts` |
| 13 | 5dd3329 | In the SSE `biased` select, the auth-recheck branch now comes before the event receiver. Shutdown stays first. | `task14 sse::revoked_session_stream_ends_at_the_recheck_despite_queued_events` |

## Behaviour changes (for the next release notes)

- A workflow with two connections into the same input is rejected at
  submission ("input port 'x:y' has 2 connections"). Before, the last one
  saved won silently. Connecting to an occupied input in the editor replaces
  the old connection.
- A nested workflow is validated like a job's own graph when the job is
  submitted. Unknown node types, unknown params and missing required inputs
  inside it are rejected up front instead of failing at run time.
- HttpRequest no longer resends POST, PATCH or other non-idempotent requests
  after a timeout or a dropped response. They are resent only when the
  connection could not be made. Errors no longer include the request body.
- StringTemplate `num_input` is limited to 64.
- Resize rejects a width or height above u32::MAX in previews too.

## Test notes

- `task14 sse::lagged_sse_subscriber_is_told_to_refetch_without_history_replay`
  creates 65 workers, which takes about 38 s, longer than the 30 s recheck.
  The recheck now runs first, and while it is pending axum sends a keep-alive
  comment. The test skips comment chunks.
- The HttpRequest tests first used fixed 200-300 ms timeouts and a 1.5 s
  server window. Under full-suite CPU load the client's connect timed out
  before the server accepted it. The test server now runs until it is stopped,
  and the cancel is triggered by the first accept.
- `streaming_executor::parallel_interpolator_tests::test_parallel_interpolator_preserves_order_and_bounds_work`
  failed once in this session. It is the known pre-existing timing flake
  recorded in `codex-project-review-2026-10-23.md`.

## Verification

```bash
export ORT_DYLIB_PATH=$PWD/lib/libonnxruntime.so
export TRT_LIBS=$HOME/miniconda3/envs/anime/lib/python3.13/site-packages/tensorrt_libs
export LD_LIBRARY_PATH=$TRT_LIBS:$PWD/lib:$HOME/miniconda3/envs/anime/lib:$LD_LIBRARY_PATH
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo clippy --locked -p videnoa-controller --all-targets --all-features -- -D warnings
cargo test -p videnoa-core -p videnoa-app -p videnoa-protocol -p videnoa-transport
cargo test -p videnoa-controller --all-features
(cd web && npx tsc -b --noEmit && npx eslint . && npx vitest run)
(cd controller-web && npx tsc -b --noEmit && npx eslint src && npx vitest run)
```

Results on 2026-11-13:
- Both clippy runs and `cargo fmt --check` were clean.
- core: 894 passed, 24 ignored.
- The controller suite passed in full.
- web: 301 tests passed. controller-web: 199 tests passed.
