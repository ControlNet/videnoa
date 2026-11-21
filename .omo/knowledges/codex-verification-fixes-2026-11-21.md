# Codex fix-verification follow-ups (2026-11-21)

Follow-up fixes across three verification rounds for `deep-review-fixes-2026-11-13.md`:
- **Round 1**: five remaining gaps and one batching regression found when
  re-verifying `3a58d9d` vs `7d8134c` (items 10 and 14-18 remain unchanged per
  the earlier scope and product decisions recorded in
  `deep-review-fixes-2026-11-13.md`).
- **Round 2**: six concurrency, filesystem-safety, UTF-8, and UI lifecycle
  follow-ups uncovered during deep audit of the Round 1 changes and surrounding
  server/UI paths.
- **Round 3**: six cross-workspace concurrency, volume-identity, frontend event
  replay, retry clock/jitter, and read-snapshot consistency fixes uncovered when
  re-auditing `e84fbf8`.

Each fix includes deterministic regression tests covering the exact reproduction
path and related edge cases.

## Round 1 summary

| Finding | Original # | Commit | Root cause | Fix & regression tests |
|---|---|---|---|---|
| D (P2) | #12 regression | `fd1b89f` | `useWorkerNames` pre-filtered incoming batched deltas against the initial `current` `Map` (`values.filter((incoming) => current.get(incoming.id) !== incoming.name)`). A round-trip rename `A -> B -> A` in one batch dropped the final `A` and left the map stuck at `B`. | Collapse incoming deltas per worker ID in chronological order (`latestByWorker`), compare final names against `current`, return `current` unchanged when net-identical, and otherwise apply `latestByWorker` onto `new Map(current)`. Tests in `controller-web/src/workers/useWorkerNames.test.tsx` cover `A -> B -> A`, `A -> B -> C`, multi-worker batches, and net-no-op `Map` reference preservation. |
| B (P2) | #9 | `8c8bda6` | `reqwest::blocking::Client` followed redirects automatically while `is_retryable_reqwest_error` treated every `err.is_connect()` as safe to retry. When a `POST` reached the origin server, received `301`/`302`/`303`/`307`/`308`, and failed to connect to the `Location` target, the retry loop replayed the original `POST` up to `max_attempts` times. | Added an attempt-scoped `RedirectTracker` inside `reqwest::redirect::Policy::custom` delegating hop limits, method/body conversion, and cross-origin header stripping to `Policy::default()`. Once any redirect hop occurs on an attempt (`redirected = true`), non-idempotent methods (`!resend_safe`) never retry on subsequent errors. Tests in `crates/core/src/nodes/http_request.rs`: `test_http_request_post_303_redirect_connect_failure_does_not_resend_post`, `test_http_request_post_307_redirect_connect_failure_does_not_resend_post`, `test_http_request_post_303_redirect_to_working_target_succeeds`, `test_http_request_get_redirect_connect_failure_retries_as_idempotent`. |
| A (P1) | #1 | `f401212` | `finish_job` updated in-memory `jobs` to `Completed`/`Failed`/`Cancelled` before `jobs.db` committed, and `unpersisted_terminal` existed only in memory with a 6-attempt cap. A hard crash before commit or a longer DB outage left publicly completed jobs reverting to `cancelled` on restart. | `finish_job` keeps in-memory status at its active pre-terminal state (`Running`/`Queued`) and retains `progress_senders` until `jobs.db` commits. On DB failure it writes an atomic `sync_all()` sidecar `<data_dir>/pending_terminal/<job_id>.json` and wakes a `Weak<AppStateInner>` background worker that retries indefinitely with capped backoff (`250ms` up to `30s`) while coordinating with `begin_shutdown`, `wait_for_jobs_to_stop`, and `delete_job_history`. Startup replays `pending_terminal/*.json` before cancelling interrupted `running` rows. Tests in `crates/core/src/server/mod.rs`, `crates/core/src/server/persistence/tests.rs`, and `crates/core/tests/server_regressions.rs`: `test_terminal_write_retries_past_six_attempts_and_heals`, `test_hard_restart_after_failed_terminal_write_recovers_from_sidecar`, `pending_terminal_sidecar_is_replayed_before_running_jobs_are_cancelled`, `terminal_persistence_failure_holds_public_running_and_recovers_on_restart`. |
| E (P2) | #6 | `f401212` | Startup staged-output cleanup inferred `output_path` statically from `VideoOutput` literals and direct `WorkflowInput` bindings, missing paths computed at runtime by upstream nodes (`PathJoiner`, `StringTemplate`, etc.). | Added SQLite schema v3 table `job_staged_files` and `StagedOutputHook` on `ExecutionContext` (propagated through `SequentialExecutor`, `WorkflowNode`, and `VideoCompileContext`). `VideoOutputNode::execute` and `EncoderConfig::reserve_temp_beside_output` record resolved target and `.videnoa-<direct\|video\|mux>` paths before/as staged files are created (preserving `.videnoa-kept`). Terminal DB transitions clear `job_staged_files` atomically; crash recovery sweeps recorded obligations alongside the static scan and deletes rows only once removal succeeds. Tests: `staged_file_records_persist_across_running_reconciliation_until_cleared`, `staged_output_hook_records_target_and_temp_file_paths`, `startup_removes_staged_outputs_for_dynamically_computed_output_path`. |
| C (P2) | #3 | `8f976f8` | `check_nested_workflow` validated inner graphs with `validate_own_nodes` in isolation without checking caller `interface_inputs` / `interface_outputs` against inner `WorkflowInput` / `WorkflowOutput` declarations, allowing port name, `PortType`, and required-input mismatches into the queue. | `check_nested_workflow` now passes caller params at every nesting depth and runs `validate_nested_interface`, checking port existence, `PortType::is_compatible`, single `WorkflowOutput` producer, connected/default/injected value sources for inner outputs and required inner inputs, and malformed/duplicate port definitions. `WorkflowNode::from_params` also inspects immediate inner `WorkflowInput` fallbacks (`default_value` or injected param) so unconnected outer `interface_inputs` backed by inner defaults remain optional unless explicitly marked `"required": true`. Tests in `crates/core/src/nodes/workflow_io.rs` and `crates/core/tests/workflow_validation.rs`: `nested_interface_output_missing_from_inner_workflow_is_rejected`, `nested_interface_port_type_and_target_input_mismatches_are_rejected`, `nested_required_workflow_input_must_be_satisfied_at_submission`, `multi_level_nested_interface_mismatch_is_caught_at_outer_validate`, `unconnected_interface_input_with_inner_default_or_injected_param_runs`. |

## Round 2 summary

| Finding | Severity | Commit | Root cause | Fix & regression tests |
|---|---|---|---|---|
| 1. Cancel/delete race against terminal commit window | P1 | `76cb2a2` | Holding in-memory `job.status` at `Running` until `jobs.db` committed opened a window between `persist_job_transition` committing `Completed` to SQLite and `finish_job` re-acquiring the `jobs` shard to publish it: a concurrent `DELETE /api/jobs/{id}` (or `begin_shutdown`) saw `Running` with no `unpersisted_terminal` entry and issued a full-row `update_job` that overwrote SQLite `status = 'completed'` back to `'running'`. In addition, `Persistence::delete_job` deleted only from `jobs`, leaving orphaned `job_staged_files` rows and pending sidecars. | `UnpersistedTerminalState` now tracks `committing` from the initial `finish_job` snapshot under the `jobs` shard until `publish_terminal_transition` updates `jobs` (or `retry_terminal_write` atomically hands off to `pending`). `delete_job_history` and `begin_shutdown` inspect `committing` and `pending` under strict lock order, release all DashMap/`Mutex` guards before SQLite I/O, and call conditional `record_cancel_request` (`WHERE id = ?1 AND status IN ('queued', 'running') AND cancel_requested_at IS NULL`) rather than full-row `update_job`; non-terminal `update_job` also guards `status IN ('queued', 'running')` and preserves cancel columns via `COALESCE`. `delete_job` deletes matching `job_staged_files` rows in the same immediate transaction and removes any pending sidecar; terminal/unloaded deletes serialize under async `job_delete_lock`. Tests: `delete_during_terminal_commit_to_publish_window_preserves_completed_state`, `shutdown_during_terminal_commit_to_publish_window_preserves_completed_state`, `record_cancel_request_and_non_terminal_update_never_clobber_terminal_or_prior_cancel`, `delete_job_cleans_up_staged_file_records_and_sidecar_for_target_job_only`. |
| 2. Shutdown missing in-flight admitted submissions | P2 | `76cb2a2` | Submission handlers released the synchronous admission lock before completing SQLite persistence, publishing into `jobs`, and spawning executors, allowing `begin_shutdown` + `wait_for_jobs_to_stop` to see `active.is_empty()` while an admitted request was mid-persistence and later spawned after shutdown started. | Replaced `admission: std::sync::Mutex<()>` with `AdmissionState { shutting_down, in_flight }`, RAII `AdmissionGuard`, and `admission_notify` across `POST /api/jobs`, unkeyed and keyed `POST /api/run`, `POST /api/jobs/{id}/rerun`, and `POST /api/batch`. `commit_admitted_jobs` atomically re-checks `shutting_down` when publishing to `jobs`; if shutdown started while SQLite writes were in flight, it rolls back unpublished DB rows and idempotency claims while holding `in_flight > 0` and returns `503 shutting_down` without spawning an executor. `wait_for_jobs_to_stop` drains `in_flight > 0` before checking `active.is_empty()`, and `run_job` uses `biased;` cancellation checks before/after permit acquisition. Tests: `shutdown_rejects_in_flight_submissions_after_db_lock_releases`, `wait_for_jobs_to_stop_drains_in_flight_single_idempotent_and_batch_submissions`, `shutdown_rolls_back_in_flight_idempotent_claim_without_leaving_key_receipt`. |
| 3. Atomic workflow save widening `0600` permissions | P2 | `76cb2a2` | `replace_workflow_file` configured `NamedTempFile` with mode `0o666`, exposing `.videnoa-save` staging files before `sync_all()` and widening existing `0600`/`0640` workflow files to `0666 & !umask` (for example `0644` under `umask 022`) on atomic `persist(path)` rename. | Keep `.videnoa-save` staging files at `0600` during write and `sync_all()`. Before `staged.persist(path)`, Unix preserves existing target permissions (`meta.permissions().mode() & 0o777`), or parses `Umask:` from `/proc/self/status` on Linux (`0o666 & !umask`, fallback `0o600`, zero calls to thread-unsafe `libc::umask`) when creating a new file; `write_new_workflow_file` also calls `sync_all()`. Tests: `test_parse_proc_status_umask_handles_valid_and_missing_lines`, `test_save_workflow_overwrite_preserves_existing_file_permissions`, `test_replace_workflow_file_preserves_0600_under_umask_022_subprocess`, `workflow_overwrite_preserves_private_file_permissions_across_atomic_replace`. |
| 4. `RunWorkflowDialog` stale workflow-list race on reopen | P2 | `8f5241a` | The `useEffect` fetching `Promise.all([listPresets(), listWorkflows()])` on dialog open had no cleanup cancellation and retained previous `workflows` across reopen/error cycles, allowing a slow response from a closed open-cycle to overwrite fresher workflow graphs, clear `loading` early, or show stale error toasts. | Added effect cleanup cancellation (`let cancelled = false; return () => { cancelled = true; };`) guarding `.then`, `.catch`, and `.finally`, and reset `workflows` to `[]` on reopen and load failure so stale graphs cannot render or submit. Tests in `web/src/pages/jobs/__tests__/RunWorkflowDialog.test.tsx`: 4 deferred-promise race tests covering late stale resolution after fresh reopen load (verifying `submitJob` receives the fresh graph), cancelled rejection while reopen pending, cancelled resolution while reopen pending, and clearing prior workflows on reopen failure. |
| 5. UTF-8 mid-codepoint split panic in staged-output sweep | P2 | `e0d7e91` | `is_staged_filename_for_owner` in `crates/core/src/server/staged_outputs.rs` called unchecked `before_marker.split_at(random_start)` (`len - 6`), panicking during startup crash recovery when a hidden `.videnoa-*` filename had a multi-byte UTF-8 character straddling byte offset `len - 6`. | Replaced `before_marker.split_at(random_start)` with `before_marker.split_at_checked(random_start)` returning `false` on non-char-boundary offsets (leaving zero unchecked byte-index string splits in `staged_outputs.rs`). Test in `staged_outputs::tests`: `multibyte_utf8_names_never_panic_and_valid_unicode_stems_still_sweep` (2-byte, 3-byte CJK, and 4-byte emoji hidden `.videnoa-*` names plus valid Unicode stems). |
| 6. Premature staged-output obligation cleanup on offline volumes | P2 | `e0d7e91` | Startup staged-output sweep treated `ErrorKind::NotFound` on missing parent directories or direct staged paths as completed cleanup, clearing `job_staged_files` rows when an external drive or network mount was offline and stranding staged files once remounted. | Added `parent_dir_or_cwd` and `parent_dir_is_accessible` (`std::fs::read_dir(dir)`). `sweep_beside_checked` returns `false` when `dir` is `NotFound` or becomes inaccessible mid-scan, and `sweep_direct_staged_file` verifies `parent_dir_is_accessible(path)` whenever `symlink_metadata` or `remove_file` returns `NotFound`, retaining `job_staged_files` records until the parent directory is online and readable. Test in `staged_outputs::tests`: `missing_parent_directory_retains_recorded_obligations_until_volume_is_online`. |

## Round 3 summary

| Finding | Severity | Root cause | Fix & regression tests |
|---|---|---|---|
| 1. Idempotency receipt visibility & unloaded-delete race before runtime acceptance | P1 | `claim_idempotent_job` inserted `status = 'queued'` into `jobs.db` before `commit_admitted_jobs` published the job into `state.inner.jobs`. During that window, `GET /api/jobs/by-idempotency-key/{key}` returned `200` while `GET /api/jobs/{id}` returned `404` (and a concurrent shutdown rollback deleted the already-reported receipt); meanwhile `DELETE /api/jobs/{id}` saw no in-memory entry and treated the in-flight row as an unloadable historical row, deleting the durable idempotency record while the job still entered memory as `201 Created`. | Added pre-acceptance persisted status `admitting`, per-key async serialization (`idempotency_key_locks`), and `AdmissionState.creating_ids`. New submissions insert `status = 'admitting'`, promote to `queued` in `commit_admitted_jobs`, and become visible to key lookups and idempotency replays only once accepted into `state.inner.jobs`. `delete_job_history` guards both `creating_ids` and `delete_unloaded_job` (`status != 'admitting'`), and startup purges any crash-interrupted uncommitted `admitting` rows. Tests: `idempotent_claim_window_hides_uncommitted_receipt_and_rolls_back_cleanly_on_shutdown`, `delete_during_idempotent_claim_window_returns_404_and_preserves_idempotency_record`, `admitting_rows_are_protected_from_unloaded_delete_and_purged_if_uncommitted_at_startup`. |
| 2. Readable empty mountpoint clearing staged-output records when unmounted | P2 | When an output was placed directly at a mountpoint root (with no subdirectory), unmounting exposed an underlying empty host directory that remained readable (`read_dir` succeeded with 0 entries), causing startup sweep to clear `job_staged_files` rows while staged files remained on the detached volume. | Bumped `jobs.db` to schema v4 (`dir_dev` and `dir_ino` on `job_staged_files`). `record_staged_file` captures the target parent directory's `(st_dev, st_ino)` identity, and startup sweep verifies `dir_identity_matches` before marking missing/empty scans as completed, retaining records until the original volume returns. Tests: `unmounted_readable_mountpoint_retains_recorded_obligations_until_original_volume_returns`, `v3_database_migrates_to_v4_and_captures_directory_identity`. |
| 3. `useTasksData` dropping off-filter SSE exits during in-flight page GET | P2 | While a filtered `GET /api/tasks` (e.g. `status=processing`) was in flight, an SSE transition to `completed` failed `matchesTaskQuery`, skipped refetching, yet advanced `appliedUpdateGeneration`; the stale GET then resolved and left the completed task stuck as `processing`. | `useTasksData` captures `requestStartGeneration` when starting each page GET, defers advancing `appliedUpdateGeneration` while the GET is in flight, and replays all deltas since `requestStartGeneration` against the resolved snapshot (merging in-place progress updates or coalescing a single refresh when filter membership changes). Tests in `controller-web/src/tasks/useTasksData.test.tsx`. |
| 4. `useWorkerNames` overwriting newer SSE renames when an older GET finishes | P2 | A slow `GET /api/workers` replaced the entire name map upon resolution without checking worker `version` or replaying SSE deltas received since the request started. | `useWorkerNames` tracks per-worker `{ name, version }`, captures `requestStartGeneration` before `GET /api/workers`, preserves higher-version entries, and replays in-flight deltas (including newly registered workers) onto the fetched baseline while preserving `Map` identity on net no-ops. Test in `controller-web/src/workers/useWorkerNames.test.tsx`. |
| 5. Controller transfer retry using pre-transfer timestamp and constant zero jitter | P2 | Transfer stages (`download`, `upload`, `cleanup`, `publish`) reused the timestamp sampled before long I/O when scheduling retries (producing already-expired `next_retry_at` deadlines after slow transfers), and `orchestration.rs` passed `JitterSample::default()` (`0`) so concurrent retries synchronized at the lower backoff bound. | Added unbiased CSPRNG `JitterSample::random()` (`0..=10_000`), injectable `TransferClock` and `JitterSource` on `TransferExecutor` (advancing by stage elapsed duration when unconfigured so deterministic fixture timestamps stay anchored), and `dispatch_recovery_live` in orchestration so every retry reads failure-time clock and draws an independent jitter sample. Tests in `crates/controller/src/lifecycle/retry.rs` and `crates/controller/tests/task12/download.rs`. |
| 6. `Store::task_page` running `COUNT(*)` and page query outside a shared transaction | P3 | Separate pool queries for `COUNT(*)` and `SELECT ... LIMIT ... OFFSET` could observe concurrent inserts/updates between the two reads (e.g. `total = 0` alongside 1 row). | `Store::task_page` executes both queries inside a single SQLite WAL read transaction (`pool().begin()`), guaranteeing a consistent point-in-time snapshot without blocking writers. Test in `crates/controller/tests/persistence_history.rs`. |

## Concurrency and persistence invariants

- **Acyclic server lock order (`crates/core/src/server/mod.rs`)**:
  - `job_delete_lock` (async `tokio::sync::Mutex`) -> `jobs` DashMap shard ->
    `unpersisted_terminal` (`std::sync::Mutex`).
  - `admission` (`std::sync::Mutex<AdmissionState>`) -> `jobs` DashMap shard.
  - Zero DashMap shard guards or `std::sync::Mutex` guards are held across
    `.await`, SQLite queries/transactions, or filesystem `sync_all()` / directory
    scans.
- **Unbroken terminal commit bridge**: `UnpersistedTerminalState.committing`
  holds the terminal transition from the moment `finish_job` snapshots the active
  job under the `jobs` shard until either `publish_terminal_transition` commits
  the terminal state to memory or `retry_terminal_write` atomically moves it to
  `pending`. Neither `DELETE /api/jobs/{id}` nor `begin_shutdown` can treat a
  committing terminal job as active, and SQLite cancel/progress writes guard
  `WHERE status IN ('queued', 'running')` so persisted terminal rows are
  monotonic.
- **Shutdown admission drain**: every job submission endpoint increments
  `AdmissionState.in_flight` via RAII `AdmissionGuard` before releasing the mutex
  for SQLite persistence and re-checks `shutting_down` when publishing into
  `jobs`. Submissions racing with `begin_shutdown` roll back unpublished SQLite
  and idempotency records before dropping `AdmissionGuard`, and
  `wait_for_jobs_to_stop` drains `in_flight == 0` before checking `active`.
- **Job terminal visibility**: `/api/jobs` and `/api/jobs/{id}` keep a finished
  job in `running` until its terminal transition commits to `jobs.db`. Under
  transient or prolonged SQLite lock/disk errors, `<data_dir>/pending_terminal/`
  preserves the outcome across process crashes without reverting a finished job
  to `cancelled`.
- **SQLite schema v3 (`jobs.db`) & offline-safe staged sweep**: `job_staged_files`
  tracks runtime staged outputs, clears atomically on terminal transition or
  `delete_job`, and retains cleanup obligations across restarts whenever a target
  parent directory is offline (`NotFound`/unreadable) until the volume returns.
- **Private workflow file modes**: atomic workflow saves keep `.videnoa-save`
  files at `0600` through `sync_all()`, preserve existing target file modes on
  overwrite, and derive new-file permissions from `/proc/self/status` `Umask:`
  without mutating process-wide `umask`.
- **Nested workflow interface validation**: workflows whose `Workflow` nodes
  declare `interface_inputs` or `interface_outputs` incompatible with the target
  workflow's `WorkflowInput` / `WorkflowOutput` nodes are rejected at validation
  / `POST /api/jobs` time across arbitrary nesting depths.
- **HttpRequest redirect retries**: non-idempotent methods (`POST`, `PATCH`,
  etc.) follow `301`/`302`/`303`/`307`/`308` redirects up to 10 hops, but never
  replay the origin request once at least one redirect hop was received.
- **Advisory gate (`3aff512`)**: RustSec published `RUSTSEC-2026-0543` marking
  `chrono` (`0.4.45`) unmaintained across all versions with no vulnerability and
  no patched `chrono` release (`cargo update -p chrono --dry-run` locks 0
  updates). Added `RUSTSEC-2026-0543` to `deny.toml` (`chrono` is used directly
  across the workspace and transitively via `iroh` / `tokio-rustls-acme` / `wmi`)
  so `cargo deny check advisories licenses` remains green locally and in CI.

## Verification

```bash
export ORT_DYLIB_PATH=$PWD/lib/libonnxruntime.so
export TRT_LIBS=$HOME/miniconda3/envs/anime/lib/python3.13/site-packages/tensorrt_libs
export LD_LIBRARY_PATH=$TRT_LIBS:$PWD/lib:$HOME/miniconda3/envs/anime/lib:${LD_LIBRARY_PATH:-}
export PKG_CONFIG_PATH=$HOME/miniconda3/envs/anime/lib/pkgconfig:${PKG_CONFIG_PATH:-}
export PATH=$PWD/bin:$PATH

cargo fmt --all -- --check
cargo check --locked --workspace --all-targets --release
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo clippy --locked -p videnoa-controller --all-targets --all-features -- -D warnings
cargo deny check advisories licenses
cargo test --locked -p videnoa-core -p videnoa-app -p videnoa-protocol -p videnoa-transport
cargo test --locked -p videnoa-controller --all-features
(cd web && npx tsc -b --noEmit && npx eslint . && npx vitest run)
(cd controller-web && npx tsc -b --noEmit && npx eslint src && npx vitest run)
node scripts/tests/validate_ci_release_workflows.test.mjs
```
