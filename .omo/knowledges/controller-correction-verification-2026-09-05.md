# Controller focused correction verification

## Scope and provenance

- User-requested correction of compute/stage-in/stage-out accounting, media-safe
  publication, and dense Tasks history controls. This supersedes conflicting
  assumptions in the original Controller plan, not its unrelated features.
- Starting branch: `dev`; clean baseline `aa21c27`, fetched and confirmed current
  with `git pull --ff-only origin dev` on 2026-09-05.
- Historical F1-F4 approvals are not evidence for these newly required invariants.
- Mock Videnoa payloads and browser fixtures are test-only synthetic data, not GPU
  execution or production media. Runtime authentication material must not appear
  in this record.

## Required behavior

- Compute occupancy is `submitting + processing` only. Stage-in is
  `reserved + uploading + staged`. Downstream states consume neither resource.
- Reservation covers unfilled compute demand plus configured prefetch, while a
  SQLite-atomic transition to `submitting` independently admits actual compute.
- One compute slot and one prefetch must permit concurrent download, processing,
  and stage-in on the same remote instance without compute oversubscription.
- Verified output moves directly from Controller temp to the exact final path
  with atomic no-replace semantics. No intermediate output-root files are allowed.
- Temp/output must permit an atomic same-filesystem move. Separate Linux bind
  mounts can reject rename with `EXDEV` even when underlying device IDs match;
  deployment needs a common mount containing separate temp/output directories.
- Matching final output with missing temp after rename recovers to cleanup;
  ambiguous or colliding final data is never overwritten or deleted.

## Unchanged-product regression evidence

- `cargo +nightly test --locked -p videnoa-core -p videnoa-app --lib --tests`,
  using the repository runtime environment: 634 passed, 10 existing ignored GPU/
  media-dependent tests, zero failures. Core/app sources are unchanged.
- `npm run build` in `web/`: passed; existing large-chunk warning remains.
- `bash scripts/tests/package_controller_test.sh`: passed synthetic Linux archive
  contract checks; this is not a build of the corrected production binary.
- `bash scripts/tests/package_controller_windows_static_test.sh`: passed static
  PowerShell contracts. Native PowerShell/Windows runtime is unavailable locally.
- `bash scripts/tests/package_dist_archive_test.sh`: passed legacy split/single
  archive, bounded-resource, and failure-propagation contracts.

## Correction gates

- `cargo +1.83.0 test --locked -p videnoa-controller --all-targets -- --test-threads=1`
  passed every target; raw evidence is `/tmp/opencode/controller-correction-rust-serial.log`.
  Subsequent changes only split test modules mechanically and corrected the UI
  overlay. The split config, Task 11 and scheduling pipeline targets passed.
- `cargo +1.83.0 fmt --all -- --check` and
  `cargo +1.83.0 clippy --locked -p videnoa-controller --all-targets --all-features -- -D warnings`
  passed again on final source. Directory LSP scans were clean but capped at 50
  files; compiler/typechecker gates cover the complete affected packages.
- In `controller-web/`, `npm run lint`, `npm test`, and Chromium passed on the
  final z-index 3 source: 116 unit tests and 50 browser tests. The isolated runner
  imports the repository Playwright config and only changes paths, output and
  preview port to 4177. Its web server runs the production TypeScript/Vite build.
- `docker build -f Dockerfile.controller -t videnoa-controller:correction-qa .`
  and `GIT_MASTER=1 bash scripts/check_controller_container.sh videnoa-controller:correction-qa --all`
  passed on the final embedded UI. Image ID:
  `sha256:9e7511945112211dc209c1c4d6817b028a572ae5a8670d9354249e48d30b4430`.
  Checks covered health, embedded SPA, persistence across restart, mount writes,
  missing config/hash, unwritable data, and outside-root task rejection.
- `CARGO_BUILD_JOBS=4 RUSTUP_TOOLCHAIN=1.83.0 bash scripts/package_controller.sh --target x86_64-unknown-linux-gnu --output-dir /tmp/opencode/videnoa-correction-archive-final`
  built and verified the corrected Linux archive with the final UI.
- Actual authenticated browser QA clicked all eight column toggles with a
  single Chinese-named persisted task and its detail inspector open. Observed
  row height 36px, picker z-index 3, no document horizontal overflow, and zero
  console errors/warnings. Source and Failure Stage filters were exercised
  against the real API. Seven final desktop/tablet/mobile captures are under
  `.omo/evidence/controller-correction-live/`.
- Final independent functional/design and CJK/visual reviewers inspected all
  seven final captures and returned PASS without blockers. Separate compute and
  publication reviews approved their focused implementations.

## Important regression names

- `scheduling_capacity::downloading_releases_compute_while_processing_and_prefetch_continue`
- `scheduling_capacity::two_compute_slots_keep_one_prefetch_without_third_remote_job`
- `capacity::staged_submission_atomically_claims_compute_capacity_and_pause`
- `capacity::overcommitted_compute_preserves_nonnegative_prefetch_budget`
- `atomic::capacity_reduction::worker_capacity_reduction_rechecks_usage_after_concurrent_compute_claim`
- `publication::output_root_contains_no_controller_file_before_final_publication`
- `publication::crash_after_rename_recovers_without_ai_replay`
- `publication::cross_filesystem_roots_are_rejected_without_copy_fallback`
- `publication::verified_source_replacement_before_rename_never_reaches_output_root`
- `publication_ambiguity`, `publication_nonregular`, and `publication_durability`
  cover contradictory evidence, legacy artifacts, FIFO/symlinks, and sync failure.
- `tasks-filters-columns.spec.ts`: server-backed filters and all eight toggles
  above the detail header with one Chinese task.

## Residual limitations and release boundary

- Default parallel Rust test execution is not consistently green: the existing
  duplicate-intake fixture sometimes returns `Database(PoolTimedOut)` inside
  `Database::open`, before HTTP requests. Isolated cases and the complete serial
  suite pass. The 100ms timeout and eight-request barrier were not weakened;
  temporary diagnostics were removed. This is a disclosed fixture limitation,
  not a claim of stable default-parallel acceptance.
- Native Windows runtime/archive execution is unavailable locally; only static
  PowerShell contracts are verified. Linux validation does not prove Windows.
- Vite reports dependency annotation warnings from Zod; the legacy web build
  retains its existing large-chunk warning.
- Docker image and Linux archive above are local QA artifacts, not a published
  release or registry image. Git delivery is recorded separately by commit/push
  results; this record does not assert remote CI or release publication.
