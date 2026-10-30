# Worker lifecycle fixes (2026-10-30)

Fixes for findings E (server half), P, N, M, R and Q of
`design-review-2026-10-30.md`. Tests named below are in
`crates/core/src/server/mod.rs` unless another file is given.

## E. Cancel keeps the job visible

- `DELETE /api/jobs/{id}` on a queued or running job requests cancellation
  (204) and the job stays listed as `queued`/`running` with
  `cancel_requested: true`, `cancel_requested_at`, `cancel_reason`
  (`user` | `shutdown` | `restart`). It becomes `cancelled` only after its
  executor exits and the GPU permit is free. DELETE on a finished job
  removes its history, as before.
- The cancel fields are omitted from JSON when there is no cancel request.
- Tests: `test_delete_job_history_requests_cancel_of_active_job_and_keeps_it`,
  `cancelling_a_running_job_keeps_it_visible_until_its_executor_exits`,
  `cancelling_a_queued_job_marks_it_cancelled_without_running_it`,
  `job_response_reports_a_cancel_request_only_when_there_is_one`;
  `crates/core/tests/server_regressions.rs::deleted_scalar_job_does_not_execute_downstream_http_request`.
- Controller (not changed): `reconcile_cancellation` never polls after
  DELETE, so it is unaffected. A job cancelled from the worker UI while a
  controller polls ends as "remote job was cancelled during worker
  restart", which is misleading for a user cancel.

## P. A panicking job fails

- `run_job` wraps compile and execution in `catch_unwind`; a panic becomes
  `failed` with "job panicked: <message>".
- Test: `a_panicking_job_is_recorded_as_failed`.

## N. Bounded admission

- `MAX_ACTIVE_JOBS = 64` queued plus running; more submissions (jobs,
  batch, rerun, idempotent create) get 503.
- A preview waits at most 2 s for the GPU, then 503 "GPU is busy".
- Jobs made only of nodes in `GPU_FREE_NODE_TYPES` skip the GPU permit.
- `/api/health` adds `gpu_slots`, `running`, `queued`, `cancelling`
  (always present, so the worker needs a controller of v0.1.9 or later;
  `docs/controller.md` compatibility notes still say older ones reject
  unknown fields).
- Tests: `health_reports_gpu_capacity_and_job_counts`,
  `job_submissions_beyond_the_queue_bound_are_refused_with_503`,
  `scalar_only_jobs_do_not_wait_for_the_gpu`,
  `only_graphs_made_of_known_scalar_nodes_skip_the_gpu`,
  `preview_processing_gives_up_quickly_while_the_gpu_is_busy`.

## M. Versioned state

- `jobs.db` uses `PRAGMA user_version` (now 2; v2 adds the cancel columns).
  A newer version is refused untouched. Rows that cannot be decoded are
  skipped with their ids logged and can be removed with DELETE.
- `config.toml` has a top-level `version` (now 2). Saving keeps keys this
  build does not know and never lowers the version.
- The worker refuses to start without its job store; rerun validates the
  stored workflow like a new submission.
- Tests: `persistence/tests.rs` (5 tests),
  `delete_removes_a_persisted_job_that_could_not_be_loaded`,
  `rerun_validates_the_stored_workflow_like_a_new_submission`,
  `the_worker_refuses_to_start_without_its_job_store`;
  `config.rs`: `saving_keeps_keys_this_build_does_not_know`,
  `saved_config_records_its_version_and_cleared_known_keys_stay_cleared`.

## R. Path anchors

- `workflows_dir` is relative to the data directory; default `workflows`.
  Config v1 values are migrated on load ("data/workflows" becomes
  "workflows", other relative values are pinned to the cwd). Workflows in
  `<cwd>/data/workflows` are copied into a custom data dir on start.
- `models_dir`, `presets_dir`, `trt_cache_dir` resolve against the first
  of cwd, exe dir and its parent that contains `presets_dir`
  (`config::asset_base_dir`).
- A relative workflow `model_path` is resolved against `models_dir` (also
  without a leading `models/`) for jobs, previews and `videnoa run`; the
  stored workflow is unchanged.
- Desktop: data dir is `<local app data>/com.videnoa.app` unless
  `VIDENOA_DATA_DIR` is set; an old `<exe_dir>/data` is renamed there once
  (kept in place if the rename fails). `crates/desktop/src/data_dir.rs`.
- Not done: the Settings page shows `server.host`/`server.port` from the
  config, which the desktop ignores (it binds 127.0.0.1 on a random port);
  showing the real address needs a web change.
- Tests: `a_relative_workflows_dir_resolves_against_the_data_dir`,
  `job_model_paths_resolve_against_the_configured_models_dir`; `config.rs`:
  `relative_state_paths_resolve_against_the_data_dir`,
  `a_version_1_config_keeps_pointing_at_its_workflows`,
  `legacy_workflows_are_copied_into_a_custom_data_dir_once`,
  `asset_paths_resolve_against_the_first_dir_with_presets`,
  `workflow_model_paths_resolve_against_models_dir`; desktop
  `data_dir::tests` (4 tests).

## Q. Shutdown and ownership

- `<data_dir>/instance.lock` is taken first in `AppState::new`; it is the
  last field of `AppStateInner`, so it is released after every store.
  Test helpers that restart a state wait for this lock.
- `AppState::begin_shutdown` (503 for new jobs and previews; cancel with
  reason `shutdown`, persisted) and `wait_for_jobs_to_stop(grace)`. The
  server calls them on SIGTERM/Ctrl-C (6 s for jobs, then 2 s for
  connections; second Ctrl-C skips), the desktop on `RunEvent::Exit`.
  `main.rs` uses `Runtime::shutdown_timeout`, because dropping a runtime
  waits for `block_in_place` executors.
- Jobs still running at exit stay `running` in the DB with the shutdown
  cancel request; the next start reconciles them and
  `server/staged_outputs.rs` removes their staged VideoOutput files.
- `subprocess::configure_child` (called by `runtime::command_for`): Unix
  process group; Windows kill-on-close job object (type-checked only).
- Dockerfile: tini as ENTRYPOINT, `STOPSIGNAL SIGTERM`.
- Tests: `a_second_instance_leaves_a_live_instance_s_data_dir_alone`,
  `a_restart_removes_staged_outputs_of_jobs_that_were_running`,
  `shutdown_stops_active_jobs_and_refuses_new_work`,
  `a_job_that_outlives_the_shutdown_grace_keeps_its_cancel_reason`;
  `staged_outputs::tests::only_this_outputs_staged_names_match`;
  `subprocess::tests::children_run_in_their_own_process_group`.
- Limits: a job restored this way has `cancel_reason: "shutdown"` but the
  generic "job restored from persisted 'running' state" error text. On
  Unix an orphaned FFmpeg is not killed when the worker exits; only its
  staged files are removed later. Outputs of nested `Workflow` nodes and
  paths computed by other nodes are not swept. Ctrl-C in a terminal no
  longer reaches FFmpeg directly. `docker run --entrypoint` bypasses tini.
- Side effects: every data dir now contains `instance.lock`; desktop dev
  runs use `~/.local/share/com.videnoa.app` unless `VIDENOA_DATA_DIR` is
  set; closing the desktop window can take up to 6 s while jobs stop.
- Manual check: start `target/debug/videnoa` on a temp data dir, start a
  second one on the same dir (exits 1 with the lock message), send SIGTERM
  to the first (exits 0, logs "Shutdown requested" then "Server stopped").

## Flaky core suite after the merge (fixed in `wave1-flaky-fix`)

- The core lib suite is fsync-bound. Sampling `/proc/<pid>/task/*/wchan`
  showed nearly all test threads in `jbd2_log_wait_commit`; with other
  builds on the same disk, a job-store write takes seconds. Count fsyncs
  with `strace -f -y -e trace=fsync,fdatasync` on the test binary.
- `begin_shutdown` used to persist while iterating the job map, so a slow
  write let a not-yet-visited queued job start and complete. It now cancels
  all jobs in memory first, then writes only the cancel columns
  (`JobsPersistence::record_cancel_request`). Test:
  `shutdown_cancels_every_job_before_it_writes_to_disk`, which holds the
  store's connection (`JobsPersistence::lock_connection`) to stall writes.
- `JobsPersistence` keeps one connection; a connection per operation
  checkpointed and deleted the WAL on every close. Schema creation and
  migration run in one transaction.
- Tests that need a job to stay running use `test_hold` with a `TestGate`
  the test opens, not a timed sleep. Wait-until helpers have a 30 s bound.
- Per-test timing: run the test binary with
  `RUSTC_BOOTSTRAP=1 <bin> -Z unstable-options --report-time`.
