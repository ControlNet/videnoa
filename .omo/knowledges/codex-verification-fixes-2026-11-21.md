# Codex fix-verification follow-ups (2026-11-21)

Follow-up fixes for the five remaining gaps and one batching regression found
when re-verifying `deep-review-fixes-2026-11-13.md` (`3a58d9d` vs `7d8134c`).
Items 10 and 14-18 remain unchanged per the earlier scope and product decisions
recorded in `deep-review-fixes-2026-11-13.md`.

Each fix includes regression tests covering the exact reproduction path and
related edge cases.

## Summary

| Finding | Original # | Commit | Root cause | Fix & regression tests |
|---|---|---|---|---|
| D (P2) | #12 regression | `fd1b89f` | `useWorkerNames` pre-filtered incoming batched deltas against the initial `current` `Map` (`values.filter((incoming) => current.get(incoming.id) !== incoming.name)`). A round-trip rename `A -> B -> A` in one batch dropped the final `A` and left the map stuck at `B`. | Collapse incoming deltas per worker ID in chronological order (`latestByWorker`), compare final names against `current`, return `current` unchanged when net-identical, and otherwise apply `latestByWorker` onto `new Map(current)`. Tests in `controller-web/src/workers/useWorkerNames.test.tsx` cover `A -> B -> A`, `A -> B -> C`, multi-worker batches, and net-no-op `Map` reference preservation. |
| B (P2) | #9 | `8c8bda6` | `reqwest::blocking::Client` followed redirects automatically while `is_retryable_reqwest_error` treated every `err.is_connect()` as safe to retry. When a `POST` reached the origin server, received `301`/`302`/`303`/`307`/`308`, and failed to connect to the `Location` target, the retry loop replayed the original `POST` up to `max_attempts` times. | Added an attempt-scoped `RedirectTracker` inside `reqwest::redirect::Policy::custom` delegating hop limits, method/body conversion, and cross-origin header stripping to `Policy::default()`. Once any redirect hop occurs on an attempt (`redirected = true`), non-idempotent methods (`!resend_safe`) never retry on subsequent errors. Tests in `crates/core/src/nodes/http_request.rs`: `test_http_request_post_303_redirect_connect_failure_does_not_resend_post`, `test_http_request_post_307_redirect_connect_failure_does_not_resend_post`, `test_http_request_post_303_redirect_to_working_target_succeeds`, `test_http_request_get_redirect_connect_failure_retries_as_idempotent`. |
| A (P1) | #1 | `f401212` | `finish_job` updated in-memory `jobs` to `Completed`/`Failed`/`Cancelled` before `jobs.db` committed, and `unpersisted_terminal` existed only in memory with a 6-attempt cap. A hard crash before commit or a longer DB outage left publicly completed jobs reverting to `cancelled` on restart. | `finish_job` keeps in-memory status at its active pre-terminal state (`Running`/`Queued`) and retains `progress_senders` until `jobs.db` commits. On DB failure it writes an atomic `sync_all()` sidecar `<data_dir>/pending_terminal/<job_id>.json` and wakes a `Weak<AppStateInner>` background worker that retries indefinitely with capped backoff (`250ms` up to `30s`) while coordinating with `begin_shutdown`, `wait_for_jobs_to_stop`, and `delete_job_history`. Startup replays `pending_terminal/*.json` before cancelling interrupted `running` rows. Tests in `crates/core/src/server/mod.rs`, `crates/core/src/server/persistence/tests.rs`, and `crates/core/tests/server_regressions.rs`: `test_terminal_write_retries_past_six_attempts_and_heals`, `test_hard_restart_after_failed_terminal_write_recovers_from_sidecar`, `pending_terminal_sidecar_is_replayed_before_running_jobs_are_cancelled`, `terminal_persistence_failure_holds_public_running_and_recovers_on_restart`. |
| E (P2) | #6 | `f401212` | Startup staged-output cleanup inferred `output_path` statically from `VideoOutput` literals and direct `WorkflowInput` bindings, missing paths computed at runtime by upstream nodes (`PathJoiner`, `StringTemplate`, etc.). | Added SQLite schema v3 table `job_staged_files` and `StagedOutputHook` on `ExecutionContext` (propagated through `SequentialExecutor`, `WorkflowNode`, and `VideoCompileContext`). `VideoOutputNode::execute` and `EncoderConfig::reserve_temp_beside_output` record resolved target and `.videnoa-<direct\|video\|mux>` paths before/as staged files are created (preserving `.videnoa-kept`). Terminal DB transitions clear `job_staged_files` atomically; crash recovery sweeps recorded obligations alongside the static scan and deletes rows only once removal succeeds. Tests: `staged_file_records_persist_across_running_reconciliation_until_cleared`, `staged_output_hook_records_target_and_temp_file_paths`, `startup_removes_staged_outputs_for_dynamically_computed_output_path`. |
| C (P2) | #3 | `8f976f8` | `check_nested_workflow` validated inner graphs with `validate_own_nodes` in isolation without checking caller `interface_inputs` / `interface_outputs` against inner `WorkflowInput` / `WorkflowOutput` declarations, allowing port name, `PortType`, and required-input mismatches into the queue. | `check_nested_workflow` now passes caller params at every nesting depth and runs `validate_nested_interface`, checking port existence, `PortType::is_compatible`, single `WorkflowOutput` producer, connected/default/injected value sources for inner outputs and required inner inputs, and malformed/duplicate port definitions. `WorkflowNode::from_params` also inspects immediate inner `WorkflowInput` fallbacks (`default_value` or injected param) so unconnected outer `interface_inputs` backed by inner defaults remain optional unless explicitly marked `"required": true`. Tests in `crates/core/src/nodes/workflow_io.rs` and `crates/core/tests/workflow_validation.rs`: `nested_interface_output_missing_from_inner_workflow_is_rejected`, `nested_interface_port_type_and_target_input_mismatches_are_rejected`, `nested_required_workflow_input_must_be_satisfied_at_submission`, `multi_level_nested_interface_mismatch_is_caught_at_outer_validate`, `unconnected_interface_input_with_inner_default_or_injected_param_runs`. |

## Behaviour and persistence notes

- **Job terminal visibility**: `/api/jobs` and `/api/jobs/{id}` keep a finished
  job in `running` until its terminal transition commits to `jobs.db`. Under
  transient or prolonged SQLite lock/disk errors, `<data_dir>/pending_terminal/`
  preserves the outcome across process crashes without reverting a finished job
  to `cancelled`.
- **SQLite schema v3 (`jobs.db`)**: adds `job_staged_files(job_id, path)` for
  runtime staged-output tracking. Existing v2 databases migrate in place on
  startup.
- **Nested workflow interface validation**: workflows whose `Workflow` nodes
  declare `interface_inputs` or `interface_outputs` incompatible with the target
  workflow's `WorkflowInput` / `WorkflowOutput` nodes are now rejected at
  validation / `POST /api/jobs` time across arbitrary nesting depths.
- **HttpRequest redirect retries**: non-idempotent methods (`POST`, `PATCH`,
  etc.) still follow `301`/`302`/`303`/`307`/`308` redirects up to 10 hops, but
  never replay the origin request once at least one redirect hop was received.
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
