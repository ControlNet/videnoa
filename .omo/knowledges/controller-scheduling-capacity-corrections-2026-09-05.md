# Controller Scheduling Capacity Corrections

## Decisions

- Compute occupancy is exactly durable `submitting` plus `processing`. `reserved`, `uploading`, and `staged` are stage-in; `remote_completed`, `downloading`, `verifying`, `publishing`, and `remote_cleanup` are downstream.
- Per-worker reservation budget is `max(compute_slots - active_compute, 0) + prefetch_per_worker`, with existing `reserved`/`uploading`/`staged` rows charged against that budget.
- `staged -> submitting` is the compute claim. Its SQLite task CAS checks persisted pause and current compute occupancy atomically. The in-memory read admission guard orders this claim against settings updates and remains held through the remote run boundary.
- A paused pre-submit checkpoint must observe `staged`, not `submitting`; otherwise a pause writer deadlocks behind a read admission guard and the task has already claimed compute.
- Upload/download transfer coordinators and one-upload-per-worker behavior were not changed.

## Red Evidence

Command:

```bash
cargo +1.83.0 test -p videnoa-controller --test task11 capacity -- --nocapture
```

Before production changes: 4 failed, 3 passed. Exact failing tests:

- `capacity::reservation_budget_covers_idle_compute_demand_plus_prefetch`: third slots2/prefetch1 reservation was absent.
- `capacity::compute_capacity_counts_only_submitting_and_processing`: `Reserved` reported `used_slots=1`, expected 0.
- `capacity::staged_submission_atomically_claims_compute_capacity_and_pause`: second staged task could not reach the intended admission race because reservation was charged as compute.
- `capacity::capacity_reduction_ignores_existing_stage_in`: reducing slots with two staged tasks returned `CapacityBelowUsage`.

The explicit overcommit edge was also run red before adding the clamp:

```bash
cargo +1.83.0 test -p videnoa-controller --test task11 overcommitted_compute_preserves_nonnegative_prefetch_budget -- --nocapture
# 1 failed: prefetched assignment was absent because idle demand became negative
```

## Green Evidence

```bash
cargo +1.83.0 test -p videnoa-controller --test task11 -- --nocapture
# 19 passed

cargo +1.83.0 test -p videnoa-controller --test scheduling_capacity -- --nocapture
# 2 passed

cargo +1.83.0 test -p videnoa-controller --test persistence_atomic -- --nocapture
# 4 passed

cargo +1.83.0 test -p videnoa-controller --test lifecycle -- --nocapture
# 21 passed

cargo +1.83.0 test -p videnoa-controller --test task20 pause:: -- --nocapture
# 1 passed

cargo +1.83.0 test -p videnoa-controller --test task20 orchestration:: -- --nocapture
# 1 passed

cargo +1.83.0 test -p videnoa-controller --test task20 one_worker:: -- --nocapture
# 1 passed

cargo +1.83.0 test -p videnoa-controller --test task20 multi_worker::three_worker_real_http_pipeline_uses_all_capacity_without_duplicates -- --nocapture
# 1 passed
```

Deterministic real-TCP regressions:

- `downloading_releases_compute_while_processing_and_prefetch_continue`: compute_slots=1/prefetch=1 observed A `Downloading`, B `Processing`, C `Uploading|Staged`, API `used_slots=1`, active download 1, two run requests total, active remote jobs 1, peak active remote jobs 1.
- `two_compute_slots_keep_one_prefetch_without_third_remote_job`: compute_slots=2/prefetch=1 observed two `Processing`, one stage-in, one `Queued`, API `used_slots=2`, and exactly two remote jobs.
- `compute_capacity_counts_only_submitting_and_processing` checks every downstream durable state as nonoccupying.

The shared Task 20 local crash matrix was aligned with direct temp-to-final publication:

```bash
cargo +1.83.0 test -p videnoa-controller --test task20 controller_restart_matrix_executes_every_local_boundary -- --nocapture
# 1 passed; exercised 7 retained local crash boundaries
```

- Removed obsolete `DestinationStaged` and `StagingVerified` matrix cases because direct publication no longer emits those checkpoints.
- Publication intent persists output size and digest without a destination staging name.
- The verified source exists through `BeforeDestinationStaging`, is renamed by `PublicationFinalized`, and its evidence remains until local cleanup.

The pre-submit checkpoint runs before admission claims compute, so both the durable task and attempt remain `staged` while blocked at `BeforeRemoteSubmit`. Assertions after remote acceptance or uncertain ownership remain `submitting`.

```bash
cargo +1.83.0 test -p videnoa-controller --test task20 timed_out_submission_waits_for_restart_before_replay -- --nocapture
# 1 passed

cargo +1.83.0 test -p videnoa-controller --test task20 controller_restart_matrix_executes_every_remote_boundary -- --nocapture
# 1 passed; exercised 9 remote crash boundaries
```

## Gate Limitation

Focused clippy command was attempted:

```bash
cargo +1.83.0 clippy -p videnoa-controller --test task11 --test scheduling_capacity -- -D warnings
```

The first attempt was blocked by a concurrent publication-peer warning in `src/scheduler/publication_finalize.rs` (`clippy::match_same_arms`). After that was fixed, the retry reached another concurrent peer warning in `src/scheduler/download_artifact.rs` (`clippy::large_enum_variant`). Both are unrelated to scheduling changes; the publication peer was notified. Targeted `rustfmt --check` and controller-wide LSP error diagnostics are clean for the scheduling edit surface.

Final integration resolved both warnings. Controller-wide strict Clippy with
`--all-targets --all-features -- -D warnings` and formatting passed. See
`controller-correction-verification-2026-09-05.md` for final gate evidence and the
separate parallel duplicate-intake fixture initialization limitation.
