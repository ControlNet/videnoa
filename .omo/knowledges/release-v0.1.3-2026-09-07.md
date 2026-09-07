# v0.1.3 release preparation and CI diagnosis

## Verified starting state

- Release candidate starts at dev `203ab0699d7562931b33f27fa8cf87d73ad856e1`.
- GitHub's latest published release was v0.1.2. Remote tags included v0.1.0,
  v0.1.1 and v0.1.2, but no v0.1.3. The older September 1 v0.1.3 evidence note
  is historical and does not establish a current release/tag; remote state was
  checked directly before preparing this release. No remote tags were overwritten.
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
and latest tags. Master was merged back into dev while the formal release ran;
dev's complete CI passed independently.

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

## Release commits and completed validation

- Candidate: `d1b4be8d759bf6a465a6d3b28317b8e8e04935bd`.
- Master release merge: `3af9606995fb809c28f7f4e4d2c49fc845acee23`.
- Dev synchronization merge: `15355bcd13b94ea9d7ed4a172d951ce5703f7fa4`.
- Initial dev CI: https://github.com/ControlNet/videnoa/actions/runs/34077162496
  (all 14 jobs passed).
- Candidate CI: https://github.com/ControlNet/videnoa/actions/runs/34077431998
  (all 14 jobs passed).
- Synchronized dev CI: https://github.com/ControlNet/videnoa/actions/runs/34078575462
  (all 14 jobs passed).
- Local validation commands above passed; the targeted Controller browser suite
  passed all 5 tests. Worker web lint passed, with 190 tests across 25 files passing.
- Staged release changes passed Secret Guard. The master merge scan flagged a
  fixed synthetic unknown-field value in `workerSchemas.test.ts`; manual review
  confirmed this is test data, not a credential.

## Public Docker verification

Both versioned images and their respective `latest` tags resolve to the same
manifest index within each repository:

- Worker: `sha256:48dfa95f9e1d8ec64301cd84ba596e5ebbb3e14df812a8b32baa942e1af25fdc`.
- Controller: `sha256:94ca885bb0302992262771a32d2fbbc1aaa72ca31196228a832780c67a8591f8`.

Both public images were pulled successfully. Controller `--version` returned
0.1.3; `bash scripts/check_controller_container.sh
controlnet/videnoa-controller:0.1.3 --all` passed source/image contracts,
zero-config setup, workspace/session/restart persistence, read-only rejection,
and external absolute media checks. Worker CLI startup, GPU passthrough, HTTP
health, embedded frontend, and `/api/about` version 0.1.3 passed.

An additional public Worker CUDA smoke used the existing RIFE v4.26 model and a
synthetic four-frame 64x64 video (not user media). It produced seven decodable
64x64 frames at 4 fps in `yuv420p10le` and exited successfully. This checks CUDA
inference; it is not a full TensorRT or large-media benchmark.

## Follow-up maintenance

Actions reports Node.js 20 deprecation notices for several existing action
versions, with execution forced onto Node.js 24. Their jobs passed; these notices
are not the Controller E2E failure described above. Action-version maintenance
is deferred rather than added to the release preparation patch.

## Published release

- Release: https://github.com/ControlNet/videnoa/releases/tag/v0.1.3
- Formal workflow: https://github.com/ControlNet/videnoa/actions/runs/34078579726
  (23 successful jobs; the expected release-skipped summary was skipped).
- Published at `2026-09-07T04:15:20Z`, with release notes updated to describe the
  Controller, NAS reliability, transfer behavior, Worker changes and persistence.
- Annotated tag object: `5b3f8cb6b1c1ed6890802a5b8b956947e22655ab`;
  peeled commit: `3af9606995fb809c28f7f4e4d2c49fc845acee23`.
- The stale local September 1 tag object was preserved at local-only
  `archive/v0.1.3-2026-09-01` before fetching the new official tag into local
  `v0.1.3`. No existing remote tag was replaced.
- Used one `gh run watch --exit-status --interval 60` process to await completion
  after initial manual status checks; its exit code was zero. Prefer this waiting
  pattern for future long CI/release runs.

Published asset SHA-256 values:

| Asset | SHA-256 |
| --- | --- |
| `videnoa-controller-v0.1.3-linux-x86_64.tar.gz` | `8ecd23fd5fcfe8ea318520fab7a7ec4cc07b41d06222fa9745daaadfda9ad3af` |
| `videnoa-controller-v0.1.3-windows-x86_64.zip` | `01697c04aca29b06dc1b79f871af01ab4ab073e40f3ff6dde8c17de8fb3660b1` |
| `videnoa-linux64-0.1.3.7z.001` | `dccc3df827ba9b6bfffe23fbb4468428507241a0a664b98d528c597c105a9c5b` |
| `videnoa-linux64-0.1.3.7z.002` | `5320d7082121f7a4a0fca2d57613482e242455834aa2fad8263c3dc94af6c282` |
| `videnoa-win64-0.1.3.7z.001` | `ca35aa2ee4195c7a1b57b6aab659c103e7fe2018bf329f5117dcf5ed66c601b3` |

All five public assets were downloaded; file sizes and SHA-256 digests matched
the GitHub release API. `7z t` passed for both complete Worker archive sets.
The Controller Linux archive passed `scripts/package_controller.sh
--verify-archive`; the extracted executable reported `videnoa-controller 0.1.3`.
The Controller Windows archive passed `unzip -tq`. Windows runtime execution was
covered by Windows CI, not by this Linux host. Local evidence is in
`/tmp/videnoa-public-release-assets-check.log` and
`/tmp/videnoa-public-v0.1.3-00uobd9a`.
