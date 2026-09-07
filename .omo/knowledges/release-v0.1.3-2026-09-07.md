# v0.1.3 release preparation and CI diagnosis

## Verified starting state

- Release candidate starts at dev `203ab0699d7562931b33f27fa8cf87d73ad856e1`.
- GitHub's latest published release was v0.1.2. Remote tags included v0.1.0,
  v0.1.1 and v0.1.2, but no v0.1.3. The older September 1 v0.1.3 evidence note
  is historical and does not establish a current release/tag; remote state was
  checked directly before preparing this release. No tags will be overwritten.
- The worktree was clean when release preparation started; concurrent changes
  from earlier sessions had already been committed to dev.

## CI failure

Run 34074432171 on `7754537` failed only Controller browser E2E: the narrow-screen
publication ambiguity test looked for `Inspect the destination and staging
artifact`, while the production copy now says `verified temporary file` and
supports manual retry. All Rust, Worker web, Worker packaging and Docker smoke
jobs passed. Controller packaging/image jobs were skipped due to the failed
frontend prerequisite. Cancelled dev runs were superseded by later pushes via
workflow concurrency, not additional test failures.

Commit `e4a600c` updates the stale assertion. The latest dev run 34077162496 already
passed its normal browser-test step during release preparation; the release
candidate must still pass all jobs before merging into master.

## Release process

Follow the prior GitFlow release pattern: branch `release/0.1.3` from dev, bump
only workspace package versions and lockfile entries, run local release-contract
checks, push the release branch, and require its entire CI to pass. Merge the
release branch into master with a merge commit to trigger Release Workflow.
Do not pre-create v0.1.3: the workflow owns the annotated tag after quality and
packaging gates. Verify GitHub assets and both Worker/Controller Docker version
and latest tags, then merge master back into dev and verify its CI.

## Scope highlights

- Controller scheduling, Web UI, batch intake/idempotency and source references.
- Capability-safe media symlink resolution, durable full-content verification
  with one hash per phase, and true HTTP transfer inactivity timeouts.
- Cross-mount publication recovery, manual ambiguity retry and detailed local
  operation/OS-error diagnostics.
- Optional service passwords, authenticated Worker access, configurable session
  lifetimes, and visible server version/source information in both frontends.
- Worker output/runtime improvements and explicit Docker data persistence.
- Rust 1.98.0 baseline. Iroh is separate future work and is not part of this release.

## Validation commands

```bash
cargo +1.98.0 metadata --locked --offline --format-version 1 --no-deps
cargo +1.98.0 fmt --all -- --check
node scripts/tests/validate_ci_release_workflows.test.mjs
bash scripts/tests/controller_docs_test.sh
npm --prefix controller-web run test:e2e -- tests/e2e/task-actions.spec.ts
```

Expected: exit zero, all four workspace packages resolve to 0.1.3, and the
previously failing browser assertion passes. Full cross-platform quality,
packaging and publication are verified by the release branch and master Actions.
