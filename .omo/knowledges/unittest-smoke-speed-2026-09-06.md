# Unit workflow failure and package smoke duration

## Findings

- Run 34027732931 failed only in the Controller Docker HTTP browser smoke. The test still expected a removed connection banner, textbox roles that changed to comboboxes, and an outdated login heading.
- That run's Linux package job took 41m16s: bundle build 7m56s, compression 30m53s, archive verification 1m33s. Windows took 27m56s: bundle build 19m54s and compression 7m14s.

## Changes

- Observe actual application SSE snapshot messages through Chromium's network events after reload and restart, and align the form selectors with the current accessible roles.
- Smoke archives use 7z store mode while retaining full real builds, dependency/model payloads, 2000 MiB volumes, and archive verification. Linux release compression still defaults to level 5; release workflow compression is unchanged.
- Windows smoke now explicitly checks archive creation exit status and runs the integrity test, in addition to checking layout.
- Package jobs cache compiled Rust dependencies in a persistent target directory outside the disposable source copy. Packaging resolves output paths through Cargo metadata so the configured target directory is honored.
- New runs cancel superseded unit workflow runs for the same workflow/ref.

## Verification

- Reproduced the stale-banner failure against a Docker image built from the current tracked source, then passed the complete real Docker/non-secure HTTP browser smoke after correcting all obsolete selectors.
- `node scripts/tests/controller_http_browser_smoke.mjs videnoa-controller:ci-fix` passes setup, authentication, CSRF, SSE, settings, worker CRUD, task creation/cancellation, and restart persistence. It uses explicitly synthetic media and an offline test worker.
- `bash scripts/tests/package_dist_archive_test.sh` passes compressed defaults, real uncompressed split/extraction, missing-volume detection, invalid compression, missing output, disk preflight, and compressor-failure checks. Its small payloads and command doubles are synthetic test fixtures.
- `node scripts/tests/validate_ci_release_workflows.test.mjs` passes positive and negative workflow contracts, including smoke compression and integrity requirements.
- Actionlint, Bash syntax, PowerShell parser validation, and `git diff --check` pass.

- p7zip 16.02 re-enables LZMA2 if `-md=16m` accompanies `-mx=0`. Store mode must omit compression dictionary parameters. The archive regression asserts the actual `Method = Copy` listing so this cannot silently regress.

## Hosted verification

- Final code run: https://github.com/ControlNet/videnoa/actions/runs/34031523129 (commit `ff8eb6a`).
- Linux package smoke passed in 7m40s versus the 41m16s baseline. Its full bundle build took 5m46s, archive creation 21s (previously 30m53s), and integrity/layout verification 2s. The dependency cache save also passed. These are cold-run observations, not a measured warm-cache comparison.
- Controller Docker HTTP browser smoke also passed on GitHub with the final code.
- Final run completed successfully: all 14 jobs passed.
- Windows package smoke passed in 21m57s versus the 27m56s baseline. Full bundle build remained 19m38s; archive creation fell from 7m14s to 5s, and integrity/layout verification took 8s. The dependency cache save passed. Future warm-build gains have not yet been measured.

Correction: a subsequent audit found that the cache action's successful save did not include the actual compilation target directory because the workspace mapping used an absolute path. See `windows-package-build-speed-2026-09-06.md` for the corrected mapping and follow-up verification. The archive timings above remain valid.
