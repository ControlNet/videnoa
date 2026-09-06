# Windows package build speed

## Measured causes

- Run 34031523129 spent 2m44s preparing Worker frontend assets and 13m57s compiling the whole release workspace. Worker archives contain only the app and desktop executables, but the workspace build also compiled Controller and built its independent frontend.
- The earlier Rust cache action reported success but its `workspaces` target was an absolute path. rust-cache treats targets as workspace-relative and joined that absolute value onto the workspace, so compiled artifacts were not actually cached. The successful cache step alone was insufficient evidence of a working compilation cache.

## Fix

- Use a relative sibling target directory in rust-cache and the corresponding absolute `CARGO_TARGET_DIR` for the package build. A new cache prefix avoids treating the incomplete old cache as an exact hit. Workflow regressions reject absolute/dynamic target mappings and mismatched Cargo/cache directories.
- Build only `videnoa-app` and `videnoa-desktop` with `--release --locked`. Their core dependency still compiles normally; Controller retains its independent archive, Docker, Rust, and browser checks.
- Windows package smoke reuses the actual `web/dist` artifact produced by the Windows web build job in the same workflow run. The optional PowerShell `-FrontendDist` input requires `index.html`; default packaging still builds its frontend.
- `scripts/tests/package_dist_windows_test.ps1` loads production helper functions via the PowerShell AST, copies real built frontend assets, checks content hashes, rejects incomplete input, and checks the package build scope. It runs in CI before the real package build.

## Verification

- `node scripts/tests/validate_ci_release_workflows.test.mjs` passes all positive and negative contracts.
- `pwsh -File scripts/tests/package_dist_windows_test.ps1 -FrontendDist web/dist` passes with real built frontend assets. It was also executed locally through the PowerShell 7.4 container.
- Actionlint, Bash syntax, and `git diff --check` pass.

## Independent CI test race

- The first verification run exposed an existing Task 20 race: the three-worker test called single-task pipeline proof while peer tasks could still be active. That proof checks the entire shared temporary root, so it could fail after one task completed even though other tasks legitimately retained workspaces.
- Wait for all three tasks to reach completion before performing per-task proof and the unchanged global cleanup assertions. The targeted regression and strict Task 20 Clippy check pass locally; the full Task 20 suite passed all 31 tests locally (`cargo +1.83.0 test --locked -p videnoa-controller --test task20`).

## Cold hosted result

- Run 34033132957: Windows package smoke passed in 16m20s, down from 21m57s. The build/assembly step dropped from 19m38s to 13m7s before any compiled cache hit.
- Both package jobs passed and saved the corrected caches. The overall run failed only on the independently identified three-worker test race; its fix is included in the next verification run.
