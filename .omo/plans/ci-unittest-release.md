# CI Workflows: unittest.yaml + release.yaml for videnoa

## TL;DR
> **Summary**: Add two GitHub Actions workflows from scratch: `unittest.yaml` for non-master branches/PRs and `release.yaml` for master-only version-gated publishing.
> **Deliverables**:
> - `.github/workflows/unittest.yaml`
> - `.github/workflows/release.yaml`
> - Master-branch release pipeline that publishes linux64/win64 distribution zips and DockerHub image (`controlnet/videnoa`)
> **Effort**: Medium
> **Parallel**: YES - 3 waves
> **Critical Path**: T2 → T3 → (T4 + T5 + T6) → T7

## Context
### Original Request
- Need two CI workflows referencing ControlNet/mita and ControlNet/tensorneko style.
- `unittest` triggers on non-master branches.
- `release` triggers only on master.
- `release` must include: linux64 distribution package, win64 distribution package, DockerHub upload.

### Interview Summary
- Workflow separation confirmed: unittest vs release.
- Trigger policy confirmed:
  - `unittest`: `push` with `branches-ignore: [master]` + `pull_request`
  - `release`: `push` on `master` only
- Release publication mode confirmed: GitHub Release only.
- DockerHub repo confirmed: `controlnet/videnoa`.
- Release cadence confirmed: publish only when version changes.
- Docker tags confirmed: `<version>` + `latest`.
- Unittest scope confirmed: Rust workspace tests + web build check.
- Release quality gate confirmed: re-run Rust workspace tests + web build on master before publish.
- GitHub release tag format confirmed: `<version>` (no `v` prefix), published release (non-draft).

### Metis Review (gaps addressed)
- Avoid comparing against “latest release” (can collide with `misc` dependency tag); gate using exact app tag existence (`<version>`).
- Add release concurrency guard to avoid overlapping master release races.
- Add crate-version coherence check (`crates/app`, `crates/core`, `crates/desktop`).
- Ensure release path is explicitly dependent on successful test/quality checks.
- Define clear partial-failure behavior and acceptance checks for publish/skip paths.

## Work Objectives
### Core Objective
Implement a deterministic CI/release workflow pair that matches requested branch triggers and publishes only when app version changes.

### Deliverables
- `unittest.yaml` with non-master/PR trigger and required checks.
- `release.yaml` with master-only trigger, version gating, linux/win package creation, DockerHub publish, and GitHub Release asset upload.
- Deterministic release asset names:
  - `videnoa-linux64-<version>.zip`
  - `videnoa-win64-<version>.zip`

### Definition of Done (verifiable conditions with commands)
- `python` static check confirms `unittest.yaml` trigger contains both `push.branches-ignore: [master]` and `pull_request`.
- `python` static check confirms `release.yaml` trigger is `push.branches: [master]` only.
- `python` static check confirms `release.yaml` contains concurrency and explicit permissions blocks.
- `python` static check confirms release job computes version from `crates/app/Cargo.toml`, verifies crate version coherence, and sets `publish=true/false` by exact tag existence (`<version>`).
- `python` static check confirms packaging jobs call:
  - `scripts/package_dist.sh --platform linux64 --source-dir "$GITHUB_WORKSPACE"`
  - `scripts/package_dist.ps1 -Platform win64 -SourceDir $env:GITHUB_WORKSPACE`
- `python` static check confirms Docker publish tags include `<version>` and `latest` for `controlnet/videnoa`.
- `python` static check confirms GitHub Release upload step references both linux64 and win64 zip artifacts.

### Must Have
- Zero existing source-code behavior changes outside workflow files.
- Version-gated release (skip publishing when `<version>` tag already exists).
- Explicit `needs` dependency graph from gate check to package/docker/release jobs.
- Reuse current packaging scripts (no reimplementation in YAML).

### Must NOT Have (guardrails, AI slop patterns, scope boundaries)
- Must NOT rely on “latest release” for gating logic.
- Must NOT change `scripts/package_dist.sh`, `scripts/package_dist.ps1`, or `Dockerfile` semantics.
- Must NOT add extra platform targets (macOS/arm) in this scope.
- Must NOT add unrelated lint/test framework migrations.

## Verification Strategy
> ZERO HUMAN INTERVENTION — all verification is agent-executed.
- Test decision: tests-after (static YAML contract checks + command-level workflow logic verification).
- QA policy: Every task includes happy-path and failure/edge-path scenarios.
- Evidence: `.omo/evidence/task-{N}-{slug}.{ext}`

## Execution Strategy
### Parallel Execution Waves
> Target: 5-8 tasks per wave. <3 per wave (except final) = under-splitting.
> Extract shared dependencies as Wave-1 tasks for max parallelism.

Wave 1: Workflow contracts and release gating foundation
- T1 trigger/permissions/concurrency scaffolding
- T2 unittest workflow checks
- T3 release version/coherence gate
- T4 linux64 package production
- T5 win64 package production

Wave 2: Release publication and observability
- T6 dockerhub publish path
- T7 github release asset publication
- T8 release outcome observability

### Dependency Matrix (full, all tasks)
| Task | Depends On | Blocks |
|---|---|---|
| T1 | none | T2, T3, T4, T5, T6, T7, T8 |
| T2 | T1 | T7, T8 |
| T3 | T1 | T4, T5, T6, T7, T8 |
| T4 | T1, T3 | T7, T8 |
| T5 | T1, T3 | T7, T8 |
| T6 | T1, T3 | T7, T8 |
| T7 | T2, T3, T4, T5, T6 | T8 |
| T8 | T2, T3, T4, T5, T6, T7 | final wave |

### Agent Dispatch Summary (wave → task count → categories)
- Wave 1 → 5 tasks → `quick`, `unspecified-low`
- Wave 2 → 3 tasks → `unspecified-low`

## TODOs
> Implementation + Test = ONE task. Never separate.
> EVERY task MUST have: Agent Profile + Parallelization + QA Scenarios.

- [x] 1. Scaffold `unittest.yaml` and `release.yaml` contracts

  **What to do**:
  - Create `.github/workflows/unittest.yaml` and `.github/workflows/release.yaml`.
  - Implement top-level trigger contracts exactly:
    - `unittest.yaml`: `push.branches-ignore: [master]` and `pull_request`.
    - `release.yaml`: `push.branches: [master]` only.
  - Add top-level `permissions` blocks with least privilege.
  - Add top-level `concurrency` in `release.yaml` to prevent overlapping master release runs.

  **Must NOT do**:
  - Do not add `workflow_dispatch` or additional branches.
  - Do not use `latest release` logic anywhere in this task.

  **Recommended Agent Profile**:
  - Category: `quick` — Reason: single-purpose YAML scaffolding.
  - Skills: [`git-master`] — Useful for clean, reviewable commit boundaries.
  - Omitted: [`playwright`] — No browser task involved.

  **Parallelization**: Can Parallel: NO | Wave 1 | Blocks: [2,3,4,5,6,7,8] | Blocked By: []

  **References** (executor has NO interview context — be exhaustive):
  - Pattern: `https://github.com/ControlNet/mita/blob/master/.github/workflows/test.yml` — non-master + PR test entrypoint style.
  - Pattern: `https://github.com/ControlNet/tensorneko/blob/master/.github/workflows/unittest.yml` — unittest naming/trigger style.
  - Pattern: `https://github.com/ControlNet/mita/blob/master/.github/workflows/release.yml` — master-only release entrypoint style.

  **Acceptance Criteria** (agent-executable only):
  - [ ] `test -f .github/workflows/unittest.yaml && test -f .github/workflows/release.yaml`
  - [ ] `python - <<'PY'
from pathlib import Path
u = Path('.github/workflows/unittest.yaml').read_text(encoding='utf-8')
r = Path('.github/workflows/release.yaml').read_text(encoding='utf-8')
assert 'branches-ignore' in u and 'master' in u and 'pull_request' in u
assert 'push:' in r and 'branches:' in r and 'master' in r and 'concurrency:' in r
print('workflow trigger skeleton checks passed')
PY`

  **QA Scenarios** (MANDATORY — task incomplete without these):
  ```
  Scenario: Trigger contract happy path
    Tool: Bash
    Steps: Run the acceptance Python script against both workflow files.
    Expected: Exit code 0 with "workflow trigger skeleton checks passed".
    Evidence: .omo/evidence/task-1-workflow-scaffold.txt

  Scenario: Trigger contract failure detection
    Tool: Bash
    Steps: Create /tmp/unittest-invalid.yaml without pull_request block, run same validator adapted to that temp file.
    Expected: Non-zero exit with assertion failure mentioning missing pull_request.
    Evidence: .omo/evidence/task-1-workflow-scaffold-error.txt
  ```

  **Commit**: NO | Message: `ci(workflows): scaffold unittest and release contracts` | Files: [`.github/workflows/unittest.yaml`, `.github/workflows/release.yaml`]

- [x] 2. Implement `unittest.yaml` jobs (Rust + Web)

  **What to do**:
  - In `unittest.yaml`, add two jobs:
    1) `rust-tests` (Ubuntu) with Rust toolchain setup, `protobuf-compiler` install, and Rust test command.
    2) `web-build-check` (Ubuntu) with Node setup and `web` build command.
  - Rust test command must run real tests for app/core scope without desktop GUI dependency:
    - `cargo test -p videnoa-core -p videnoa-app`
  - Web build command must run from `web/`:
    - `npm ci --no-fund`
    - `npm run build`

  **Must NOT do**:
  - Do not run Docker publish or release steps in unittest workflow.
  - Do not include `videnoa-desktop` runtime launch in unittest.

  **Recommended Agent Profile**:
  - Category: `quick` — Reason: focused workflow job authoring.
  - Skills: [`git-master`] — Keep commit atomic and readable.
  - Omitted: [`playwright`] — No UI browser verification required.

  **Parallelization**: Can Parallel: NO | Wave 1 | Blocks: [7,8] | Blocked By: [1]

  **References** (executor has NO interview context — be exhaustive):
  - API/Type: `Cargo.toml:1-3` — workspace members are `core`, `app`, `desktop`.
  - Pattern: `crates/core/tests/crash_hook.rs:72-110` — confirms deterministic Rust test coverage exists.
  - Pattern: `web/package.json:6-12` — `build` script is `tsc -b && vite build`.
  - Pattern: `https://github.com/ControlNet/mita/blob/master/.github/workflows/test.yml` — split test workflow structure.

  **Acceptance Criteria** (agent-executable only):
  - [ ] `python - <<'PY'
from pathlib import Path
u = Path('.github/workflows/unittest.yaml').read_text(encoding='utf-8')
assert 'rust-tests:' in u
assert 'web-build-check:' in u
assert 'cargo test -p videnoa-core -p videnoa-app' in u
assert 'npm ci --no-fund' in u and 'npm run build' in u
print('unittest job contract checks passed')
PY`

  **QA Scenarios** (MANDATORY — task incomplete without these):
  ```
  Scenario: Unittest command happy path
    Tool: Bash
    Steps: Run `cargo test -p videnoa-core -p videnoa-app`; then run `cd web && npm ci --no-fund && npm run build`.
    Expected: Both commands exit 0.
    Evidence: .omo/evidence/task-2-unittest-jobs.txt

  Scenario: Rust job failure path
    Tool: Bash
    Steps: Run `cargo test -p does-not-exist` to ensure workflow would fail on invalid package target.
    Expected: Non-zero exit with cargo package-not-found message.
    Evidence: .omo/evidence/task-2-unittest-jobs-error.txt
  ```

  **Commit**: YES | Message: `ci(unittest): add non-master rust and web checks` | Files: [`.github/workflows/unittest.yaml`]

- [x] 3. Implement release version gate + crate-version coherence + quality gate

  **What to do**:
  - In `release.yaml`, add `version-gate` job that:
    - Reads versions from `crates/app/Cargo.toml`, `crates/core/Cargo.toml`, `crates/desktop/Cargo.toml`.
    - Fails if versions are not identical.
    - Uses the app version as release tag value `<version>` (no `v` prefix).
    - Checks exact tag existence using `gh release view "<version>"`.
    - Emits outputs: `version`, `tag`, `publish` (`true`/`false`).
  - Add `quality-gate` job (master release preflight) that runs only when `publish == true` and executes:
    - `cargo test -p videnoa-core -p videnoa-app`
    - `cd web && npm ci --no-fund && npm run build`

  **Must NOT do**:
  - Do not compare against “latest release”.
  - Do not publish from this task.

  **Recommended Agent Profile**:
  - Category: `unspecified-low` — Reason: logic-heavy YAML scripting with output contracts.
  - Skills: [`git-master`] — preserve clear, reviewable diffs.
  - Omitted: [`playwright`] — non-UI logic.

  **Parallelization**: Can Parallel: NO | Wave 1 | Blocks: [4,5,6,7,8] | Blocked By: [1]

  **References** (executor has NO interview context — be exhaustive):
  - API/Type: `crates/app/Cargo.toml:1-4` — app package version source.
  - API/Type: `crates/core/Cargo.toml:1-4` — core package version must match.
  - API/Type: `crates/desktop/Cargo.toml:1-4` — desktop package version must match.
  - Pattern: `https://github.com/ControlNet/mita/blob/master/.github/workflows/server-release.yml` — release gate + test-before-publish pattern.
  - Pattern: `https://github.com/ControlNet/tensorneko/blob/master/.github/workflows/release.yml` — version-check gating pattern.

  **Acceptance Criteria** (agent-executable only):
  - [ ] `python - <<'PY'
from pathlib import Path
r = Path('.github/workflows/release.yaml').read_text(encoding='utf-8')
assert 'version-gate:' in r
assert 'quality-gate:' in r
assert 'crates/app/Cargo.toml' in r and 'crates/core/Cargo.toml' in r and 'crates/desktop/Cargo.toml' in r
assert 'gh release view' in r
assert 'publish' in r and 'outputs' in r
print('release gate checks passed')
PY`

  **QA Scenarios** (MANDATORY — task incomplete without these):
  ```
  Scenario: Version gate happy path
    Tool: Bash
    Steps: Run the same Python tomllib extraction logic locally to read all three Cargo.toml versions and assert equality.
    Expected: Exit code 0 and printed version/tag without `v` prefix.
    Evidence: .omo/evidence/task-3-release-gate.txt

  Scenario: Version mismatch edge case
    Tool: Bash
    Steps: Copy Cargo.toml files to /tmp, alter one copied version value, run the extraction validator against /tmp copies.
    Expected: Non-zero exit with mismatch assertion.
    Evidence: .omo/evidence/task-3-release-gate-error.txt
  ```

  **Commit**: NO | Message: `ci(release): add version and quality gate` | Files: [`.github/workflows/release.yaml`]

- [x] 4. Add `package-linux64` release producer job

  **What to do**:
  - Add `package-linux64` job in `release.yaml` with `needs: [version-gate, quality-gate]` and `if: publish == true`.
  - Use `scripts/package_dist.sh` with explicit source and platform:
    - `bash scripts/package_dist.sh --platform linux64 --release-tag misc --output-dir "$RUNNER_TEMP/dist-linux64" --source-dir "$GITHUB_WORKSPACE" --force`
  - Zip output folder as `videnoa-linux64-<version>.zip`.
  - Upload zip as artifact for downstream release aggregation.

  **Must NOT do**:
  - Do not hardcode clone-based build path inside CI (must use `--source-dir`).
  - Do not rename internal bundle folder away from `videnoa`.

  **Recommended Agent Profile**:
  - Category: `quick` — Reason: deterministic command wiring to existing script.
  - Skills: [`git-master`] — keep change isolated.
  - Omitted: [`playwright`] — no browser work.

  **Parallelization**: Can Parallel: YES | Wave 1 | Blocks: [7,8] | Blocked By: [1,3]

  **References** (executor has NO interview context — be exhaustive):
  - Pattern: `scripts/package_dist.sh:17-47` — supported flags and defaults.
  - Pattern: `scripts/package_dist.sh:317-333` — `linux64` asset names and binary naming.
  - Pattern: `scripts/package_dist.sh:364-373` — `--source-dir` handling for current checkout.
  - Pattern: `scripts/package_dist.sh:401-426` — expected built binaries and final bundle layout validation.

  **Acceptance Criteria** (agent-executable only):
  - [ ] `python - <<'PY'
from pathlib import Path
r = Path('.github/workflows/release.yaml').read_text(encoding='utf-8')
assert 'package-linux64:' in r
assert 'scripts/package_dist.sh --platform linux64' in r
assert '--source-dir "$GITHUB_WORKSPACE"' in r or '--source-dir ${GITHUB_WORKSPACE}' in r
assert 'videnoa-linux64-' in r and '.zip' in r
print('linux64 packaging contract checks passed')
PY`

  **QA Scenarios** (MANDATORY — task incomplete without these):
  ```
  Scenario: Linux packaging happy path
    Tool: Bash
    Steps: Run the exact package_dist.sh command with linux64 and source-dir against current workspace, then zip the resulting videnoa folder.
    Expected: `videnoa-linux64-<version>.zip` exists and script exits 0.
    Evidence: .omo/evidence/task-4-package-linux64.txt

  Scenario: Unsupported platform failure path
    Tool: Bash
    Steps: Run `bash scripts/package_dist.sh --platform invalid --source-dir "$PWD" --output-dir /tmp --force`.
    Expected: Non-zero exit with `unsupported platform` message.
    Evidence: .omo/evidence/task-4-package-linux64-error.txt
  ```

  **Commit**: NO | Message: `ci(release): add linux64 packaging job` | Files: [`.github/workflows/release.yaml`]

- [x] 5. Add `package-win64` release producer job

  **What to do**:
  - Add `package-win64` job in `release.yaml` on `windows-latest` with `needs: [version-gate, quality-gate]` and `if: publish == true`.
  - Use `scripts/package_dist.ps1` command with explicit source and platform:
    - `powershell -File scripts/package_dist.ps1 -Platform win64 -ReleaseTag misc -OutputDir "$env:RUNNER_TEMP\dist-win64" -SourceDir "$env:GITHUB_WORKSPACE" -Force`
  - Archive output as `videnoa-win64-<version>.zip`.
  - Upload zip as artifact for downstream release aggregation.

  **Must NOT do**:
  - Do not rely on platform auto-detection in CI (set `-Platform win64` explicitly).
  - Do not use non-WinPS-compatible syntax in workflow PowerShell commands.

  **Recommended Agent Profile**:
  - Category: `quick` — Reason: straightforward Windows job wiring.
  - Skills: [`git-master`] — preserve clean commit history.
  - Omitted: [`playwright`] — no UI scope.

  **Parallelization**: Can Parallel: YES | Wave 1 | Blocks: [7,8] | Blocked By: [1,3]

  **References** (executor has NO interview context — be exhaustive):
  - Pattern: `scripts/package_dist.ps1:20-53` — parameters and WinPS usage examples.
  - Pattern: `scripts/package_dist.ps1:339-357` — `win64` asset names and executable suffix.
  - Pattern: `scripts/package_dist.ps1:402-415` — `-SourceDir` behavior.
  - Pattern: `scripts/package_dist.ps1:474-476` — bundle success path.

  **Acceptance Criteria** (agent-executable only):
  - [ ] `python - <<'PY'
from pathlib import Path
r = Path('.github/workflows/release.yaml').read_text(encoding='utf-8')
assert 'package-win64:' in r
assert 'scripts/package_dist.ps1' in r and '-Platform win64' in r
assert '-SourceDir "$env:GITHUB_WORKSPACE"' in r or '-SourceDir $env:GITHUB_WORKSPACE' in r
assert 'videnoa-win64-' in r and '.zip' in r
print('win64 packaging contract checks passed')
PY`

  **QA Scenarios** (MANDATORY — task incomplete without these):
  ```
  Scenario: Windows packaging happy path
    Tool: Bash
    Steps: On Windows runner, execute the exact PowerShell packaging command and compress result to `videnoa-win64-<version>.zip`.
    Expected: Zip exists and packaging exits 0.
    Evidence: .omo/evidence/task-5-package-win64.txt

  Scenario: Invalid platform failure path
    Tool: Bash
    Steps: Run `powershell -File scripts/package_dist.ps1 -Platform badvalue -OutputDir $env:TEMP\x -SourceDir $env:GITHUB_WORKSPACE -Force`.
    Expected: Non-zero exit from `ValidateSet` error.
    Evidence: .omo/evidence/task-5-package-win64-error.txt
  ```

  **Commit**: NO | Message: `ci(release): add win64 packaging job` | Files: [`.github/workflows/release.yaml`]

- [x] 6. Add `dockerhub-publish` job for `controlnet/videnoa`

  **What to do**:
  - Add `dockerhub-publish` job in `release.yaml` with `needs: [version-gate, quality-gate]` and `if: publish == true`.
  - Add credential preflight step that fails with explicit message when secrets are empty:
    - `DOCKERHUB_USERNAME`
    - `DOCKERHUB_TOKEN`
  - Configure Docker buildx and login actions.
  - Publish tags:
    - `controlnet/videnoa:<version>`
    - `controlnet/videnoa:latest`

  **Must NOT do**:
  - Do not push to other registries.
  - Do not publish `sha` tag in this scope.

  **Recommended Agent Profile**:
  - Category: `unspecified-low` — Reason: registry auth and publish wiring.
  - Skills: [`git-master`] — keep release YAML changes cohesive.
  - Omitted: [`playwright`] — no browser automation needed.

  **Parallelization**: Can Parallel: YES | Wave 2 | Blocks: [7,8] | Blocked By: [1,3]

  **References** (executor has NO interview context — be exhaustive):
  - Pattern: `Dockerfile:25-126` — canonical build context and runtime image target.
  - Pattern: `README.md:73-79` — documented Docker build/run baseline.
  - Pattern: `https://github.com/ControlNet/mita/blob/master/.github/workflows/server-release.yml` — DockerHub login/build-push pattern.

  **Acceptance Criteria** (agent-executable only):
  - [ ] `python - <<'PY'
from pathlib import Path
r = Path('.github/workflows/release.yaml').read_text(encoding='utf-8')
assert 'dockerhub-publish:' in r
assert 'DOCKERHUB_USERNAME' in r and 'DOCKERHUB_TOKEN' in r
assert 'docker/login-action' in r
assert 'docker/build-push-action' in r
assert 'controlnet/videnoa:${{ needs.version-gate.outputs.version }}' in r
assert 'controlnet/videnoa:latest' in r
print('docker publish contract checks passed')
PY`

  **QA Scenarios** (MANDATORY — task incomplete without these):
  ```
  Scenario: Docker publish happy path
    Tool: Bash
    Steps: Trigger release path with valid DockerHub secrets; inspect job log for successful push of `<version>` and `latest`.
    Expected: Both tags pushed without auth/build errors.
    Evidence: .omo/evidence/task-6-docker-publish.txt

  Scenario: Missing credential failure path
    Tool: Bash
    Steps: Run workflow in test repo/fork with one DockerHub secret intentionally unset.
    Expected: Preflight step fails early with explicit missing-secret message before build-push.
    Evidence: .omo/evidence/task-6-docker-publish-error.txt
  ```

  **Commit**: NO | Message: `ci(release): add dockerhub publish job` | Files: [`.github/workflows/release.yaml`]

- [x] 7. Add `github-release` aggregation and asset publication job

  **What to do**:
  - Add `github-release` job in `release.yaml` with `needs: [version-gate, quality-gate, package-linux64, package-win64, dockerhub-publish]` and `if: publish == true`.
  - Download linux/win build artifacts from prior jobs.
  - Create published GitHub Release with tag `<version>` (no `v` prefix) and upload:
    - `videnoa-linux64-<version>.zip`
    - `videnoa-win64-<version>.zip`
  - Use `permissions: contents: write` for this job only.

  **Must NOT do**:
  - Do not create draft release.
  - Do not publish when tag already exists (`publish == false` must skip job).

  **Recommended Agent Profile**:
  - Category: `unspecified-low` — Reason: release-asset orchestration.
  - Skills: [`github-cli`, `git-master`] — release command correctness and clean git flow.
  - Omitted: [`playwright`] — not applicable.

  **Parallelization**: Can Parallel: NO | Wave 2 | Blocks: [8] | Blocked By: [2,3,4,5,6]

  **References** (executor has NO interview context — be exhaustive):
  - Pattern: `https://github.com/ControlNet/tensorneko/blob/master/.github/workflows/release.yml` — artifact download/merge before release publication.
  - Pattern: `https://github.com/ControlNet/mita/blob/master/.github/workflows/server-release.yml` — release orchestration after quality checks.

  **Acceptance Criteria** (agent-executable only):
  - [ ] `python - <<'PY'
from pathlib import Path
r = Path('.github/workflows/release.yaml').read_text(encoding='utf-8')
assert 'github-release:' in r
assert 'needs: [version-gate, quality-gate, package-linux64, package-win64, dockerhub-publish]' in r
assert 'videnoa-linux64-' in r and 'videnoa-win64-' in r
assert 'gh release create' in r or 'softprops/action-gh-release' in r
assert 'draft: true' not in r
print('github release contract checks passed')
PY`

  **QA Scenarios** (MANDATORY — task incomplete without these):
  ```
  Scenario: GitHub release happy path
    Tool: Bash
    Steps: After publish run, execute `gh release view "<version>" --json tagName,isDraft,assets`.
    Expected: `tagName` equals `<version>`, `isDraft` is false, both linux/win zip assets present.
    Evidence: .omo/evidence/task-7-github-release.txt

  Scenario: Existing tag skip path
    Tool: Bash
    Steps: Re-run release workflow for same master commit/version where tag already exists.
    Expected: `version-gate` sets `publish=false`; `github-release` job is skipped.
    Evidence: .omo/evidence/task-7-github-release-error.txt
  ```

  **Commit**: YES | Message: `ci(release): add master-gated package, docker, and github release pipeline` | Files: [`.github/workflows/release.yaml`]

- [x] 8. Add explicit release outcome observability and contract lock checks

  **What to do**:
  - Add `release-skipped` job with `if: needs.version-gate.outputs.publish != 'true'` to emit clear summary when version unchanged.
  - Add `release-verify` job that runs after `github-release` (publish path) and validates:
    - GitHub release exists with both assets.
    - DockerHub tags `<version>` and `latest` are queryable.
  - Add step summary output (`$GITHUB_STEP_SUMMARY`) for tag, publish flag, and outcome.

  **Must NOT do**:
  - Do not mark workflow successful without either publish verification or explicit skip rationale.
  - Do not add human/manual approval gates.

  **Recommended Agent Profile**:
  - Category: `unspecified-low` — Reason: final workflow reliability and diagnostics.
  - Skills: [`github-cli`] — direct release verification commands.
  - Omitted: [`playwright`] — no UI interactions.

  **Parallelization**: Can Parallel: NO | Wave 2 | Blocks: [final wave] | Blocked By: [2,3,4,5,6,7]

  **References** (executor has NO interview context — be exhaustive):
  - Pattern: `https://github.com/ControlNet/tensorneko/blob/master/.github/workflows/release.yml` — final release orchestration and artifact integrity flow.
  - Pattern: `https://github.com/ControlNet/mita/blob/master/.github/workflows/server-release.yml` — release completion checks after test gate.

  **Acceptance Criteria** (agent-executable only):
  - [ ] `python - <<'PY'
from pathlib import Path
r = Path('.github/workflows/release.yaml').read_text(encoding='utf-8')
assert 'release-skipped:' in r
assert 'release-verify:' in r
assert 'GITHUB_STEP_SUMMARY' in r
assert 'gh release view' in r
assert 'hub.docker.com' in r or 'docker pull controlnet/videnoa' in r
print('release observability checks passed')
PY`

  **QA Scenarios** (MANDATORY — task incomplete without these):
  ```
  Scenario: Publish verification happy path
    Tool: Bash
    Steps: Run release workflow on a new version, then run `gh release view "<version>" --json assets` and query DockerHub tag endpoints for `<version>` and `latest`.
    Expected: Both checks succeed; summary states publish=true and verification passed.
    Evidence: .omo/evidence/task-8-release-observability.txt

  Scenario: Version-unchanged skip observability
    Tool: Bash
    Steps: Re-run release workflow with unchanged version.
    Expected: `release-skipped` executes, `release-verify` is skipped, summary states publish=false and reason is existing tag.
    Evidence: .omo/evidence/task-8-release-observability-error.txt
  ```

  **Commit**: NO | Message: `ci(release): add release skip/verify observability` | Files: [`.github/workflows/release.yaml`]

## Final Verification Wave (4 parallel agents, ALL must APPROVE)
- [x] F1. Plan Compliance Audit — oracle
- [x] F2. Code Quality Review — unspecified-high
- [x] F3. Real Manual QA — unspecified-high (+ playwright if UI)
- [x] F4. Scope Fidelity Check — deep

## Commit Strategy
- Commit A: `ci(unittest): add non-master rust and web checks`
- Commit B: `ci(release): add master-gated package, docker, and release pipeline`

## Success Criteria
- `unittest.yaml` and `release.yaml` exist under `.github/workflows/` and satisfy all trigger contracts.
- Release publishes only when version changes (`<version>` absent/present path works).
- Release produces linux64/win64 zip assets and publishes Docker tags `<version>` + `latest` to `controlnet/videnoa`.
- All acceptance criteria checks can be executed non-interactively.
