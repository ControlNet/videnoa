# Controller progress and cross-mount move fallback

## Scope and authorization

The user requested backend-only fixes and explicitly accepted visibility of an
incomplete final filename during cross-mount copy fallback. This supersedes the
previous unconditional Jellyfin complete-file visibility contract for EXDEV only.
The primary checkout contained ongoing user frontend edits; development was done
in `/tmp/videnoa-controller-backend` from origin/dev `236e6e6`.

## Implementation

- Processing recovery now consumes remote JobProgress only after validating job
  identity. Task/current-attempt progress is updated in one version-guarded SQLite
  transaction. Cancellation wins stale polls; a failed attempt write rolls back
  the task write. DurableChange wakes the existing SSE hub. Unchanged samples do
  not churn versions/events. Completion sets 100 percent and zero ETA.
- Intake no longer rejects output based on device mismatch. No-replace atomic
  rename remains first choice; only the typed EXDEV result invokes copying.
- Fallback exclusively creates the final output (no sibling staging). It fsyncs
  and verifies size/SHA-256 before removing the private verified source. Output
  path/parent descriptor checks and no-follow opens remain enforced.
- Private `publication-copy.evidence` contains magic, destination device/inode,
  expected size and SHA-256. Interrupted copies require matching ownership and an
  exact verified-source prefix, then append only the remainder. Replaced/corrupt
  outputs fail ambiguous and preserve the source. The narrow crash gap between
  destination creation and marker persistence conservatively requires intervention.
- Forward migration 0009 makes only legacy EXDEV publication failures with durable
  verified-output evidence retryable. They remain Failed until explicit Retry,
  which resumes publication on the same attempt without AI recomputation.
- Config/auth ownership, transfer pools, scheduler admission and frontend are unchanged.

## Validation and reproduction

```bash
cargo +1.83.0 fmt --all -- --check
cargo +1.83.0 clippy --locked -p videnoa-controller --all-targets --all-features -- -D warnings
cargo +1.83.0 test --locked -p videnoa-controller --all-targets
python3 scripts/tests/controller_mount_publication_smoke.py --image videnoa-controller:backend-qa
bash scripts/check_controller_container.sh videnoa-controller:backend-qa --all
python3 scripts/tests/controller_architecture_smoke.py --binary "$PWD/target/debug/videnoa-controller"
bash scripts/tests/controller_docs_test.sh
```

Expected: all Rust tests/checks pass; smoke scripts print PASS. The mount smoke
uses a synthetic HTTP Worker and synthetic bytes, not valid video/GPU processing.
It exercises the real Controller API/pipeline with separate data/media bind mounts
on the same device, checks live progress/FPS/ETA, final bytes, local/remote cleanup,
and completed-task persistence across restart with one remote submission.
Linux task13 tests separately exercise different devices via `/dev/shm`, four
copy crash checkpoints, no-clobber, corrupt/replaced/symlink output, replacement
after verification, and legacy failure upgrade/retry. Task13 has 42 passing tests.

The isolated worktree uses the shared primary `target` as CARGO_TARGET_DIR. A first
full-suite run stopped because the process test could not find the isolated UI
dist directory. Supplying existing deployed static assets fixed the environment;
no frontend source/build work was performed. The normal fmt command does not
recurse implementation modules behind include!(module_topology.rs), so changed
implementation files were also formatted explicitly with Rust 1.83 rustfmt.

Final validation: full Controller all-targets suite passed with default parallel
test execution; the extra publication test added during that run passed in the
subsequent 42-test task13 run. Fmt, all-target/all-feature clippy with warnings
denied, docs checks, all container smoke checks, architecture smoke, same-device
bind-mount smoke, and staged Secret Guard scan passed. No frontend sources were
changed, so frontend lint/build/E2E were not run against the user's ongoing edits.

## Local test image and deployment

`videnoa-controller:backend-qa` was built in Docker with Rust 1.83 and the existing
deployed frontend static assets; only the backend executable differs from the
previous image. Image ID: `5c89124367accfdc43789eb4ef97cc9791447b6e8f6d63a0310c4d5146a02d1d`.
The temporary build recipe is `/tmp/Dockerfile.controller-backend-qa`.
The user's running `videnoa-controller-test` container, live database, media and
old failed task were not changed. Migration/retry only takes effect when that
deployment is upgraded and the operator explicitly retries the old EXDEV task.
