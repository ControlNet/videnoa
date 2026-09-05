# F4 Packaging, Release, Regression, and Repository-Landing Audit

Audit date: 2026-09-05

Audited implementation tip: `d8830fa55cf54d504eafbc768747e57f3d2dcadf`

Comparison baseline: `67620f5b45d54205b44861e3cc5d59e88724e998`

Hosted authority: [GitHub Actions run 33919997302](https://github.com/ControlNet/videnoa/actions/runs/33919997302)

## Decision

The exact current pushed tip is rejected for F4. Run `33919997302` is the correct completed `push` run for `dev` at head SHA `d8830fa55cf54d504eafbc768747e57f3d2dcadf`, but its conclusion is `failure`: 13 of 14 jobs succeeded and the required legacy `Package smoke (Linux)` job failed while creating the split archive.

The Controller Linux archive, native Windows archive, Controller image/content, Controller Rust, Controller Web, fault/load/security, legacy Windows package, legacy Docker, and cross-platform existing-product Rust/Web jobs all passed. Local contract checks also passed. Those successes cannot override an exact-tip required hosted job failure. Run `33910972161` is historical evidence for `67620f5` only and is not used as current authority.

## Audit-Start Provenance and Landing State

Before replacing this report:

- `git status --porcelain=v1 --branch` reported `## dev...origin/dev` with no changed paths.
- `HEAD` and the local `origin/dev` tracking ref both resolved to `d8830fa55cf54d504eafbc768747e57f3d2dcadf`.
- `git rev-list --left-right --count HEAD...origin/dev` reported `0 0`.
- read-only `git ls-remote origin refs/heads/dev` independently reported `d8830fa55cf54d504eafbc768747e57f3d2dcadf`.
- no pull, push, commit, stash, reset, restore, checkout, workflow rerun, release action, or registry write was performed by this audit.

The branch was therefore clean, pushed, and up to date at audit start. The rejection is caused by exact-tip CI, not repository provenance.

## Exact-Tip Hosted Regression

Run `33919997302` completed with conclusion `failure` for a `push` event at exact head SHA `d8830fa55cf54d504eafbc768747e57f3d2dcadf`.

| Required job | Job ID | Result |
|---|---:|---|
| Workflow contracts | `101175899842` | PASS |
| Web build check (Ubuntu) | `101175899980` | PASS |
| Rust tests (Ubuntu) | `101175900036` | PASS |
| Controller web quality and E2E | `101175900048` | PASS |
| Controller Rust quality and tests | `101175900066` | PASS |
| Web build check (Windows) | `101175900074` | PASS |
| Rust tests (Windows) | `101175900102` | PASS |
| Package smoke (Windows) | `101176805366` | PASS |
| Docker build smoke | `101176805442` | PASS |
| Package smoke (Linux) | `101176805555` | **FAIL** |
| Controller fault and load suites | `101178383467` | PASS |
| Controller archive smoke (Linux) | `101178383474` | PASS |
| Controller archive smoke (Windows) | `101178383529` | PASS |
| Controller image and content smoke | `101178383579` | PASS |

The failing Linux job passed checkout, toolchain setup, dependency installation, the legacy archive contract suite, real legacy bundle construction, Linux runtime compatibility, and cache reclamation. The `Create split archive (2000MB volumes)` step then scanned 5 folders, 38 files, and `5,335,491,267` bytes, started creating `videnoa-linux64-smoke.7z`, and terminated with p7zip 16.02 `System ERROR: E_FAIL`, exit code `2`. Archive verification did not run. Available space was reported as `84,695,440 KiB`, above the helper's `5,276,028 KiB` preflight requirement, so this exact run does not establish a successful legacy Linux archive output.

GitHub's artifacts API reports `total_count: 0` for this push run. No retained downloadable package is claimed.

## Post-Baseline Commit Audit

The commits after `67620f5` are:

- `90561edafb7df40af084c3ffe8a19bebf909c3c9`: shell CSS and Playwright containment regression coverage.
- `6c952af399ec7952d17272839b53e3abb97e1428`: shell-remediation notepad evidence.
- `590d79ce0fb5f8fedf9cbb345dcfbc8bd234cfa3`: Final Wave report refresh.
- `d8830fa55cf54d504eafbc768747e57f3d2dcadf`: Final Wave plan checkbox updates.

The shell change fixes a fixed-height application frame, makes desktop Settings wheel scrolling remain within `.shell-main`, and moves narrow logout alerts into a dedicated grid row so they do not cover enabled controls. Its test change adds only browser assertions for those behaviors.

`git diff --name-only 67620f5..HEAD` contains only the CSS/test pair plus F1/F2/F4 evidence, two notepads, and the plan. A protected-path `git diff --quiet` passed for `.github/workflows`, both Dockerfiles, Controller and legacy packaging scripts, `Cargo.toml`, `Cargo.lock`, Controller/package manifests, and both Web package manifests. `git diff --check 67620f5..HEAD` passed. Therefore these four commits do not change workflows, Dockerfiles, packaging scripts, artifact names, image tags, versions, dependencies, or legacy product layouts.

## Dedicated Controller Delivery Contracts

The successful exact-tip Controller image job built `Dockerfile.controller` and ran `scripts/check_controller_container.sh --all`. The source and hosted image checks require an isolated Node frontend stage, Rust 1.83 builder, Debian bookworm-slim runtime, numeric non-root `10001:10001`, `videnoa-controller` entrypoint, embedded SPA, healthcheck, persistent/NAS mounts, restart persistence, explicit startup failures, configured-root rejection, and no legacy binary, Node/npm, models, ONNX Runtime, CUDA, cuDNN, TensorRT, or NVIDIA runtime content.

The successful exact-tip Linux Controller archive job built and verified:

- `videnoa-controller-v0.1.2-linux-x86_64.tar.gz`;
- one root named `videnoa-controller-v0.1.2-linux-x86_64`;
- exactly `LICENSE`, `README-controller.md`, `controller.example.toml`, and `videnoa-controller`.

The successful exact-tip native Windows Controller archive job built and verified:

- `videnoa-controller-v0.1.2-windows-x86_64.zip`;
- one root named `videnoa-controller-v0.1.2-windows-x86_64`;
- exactly `LICENSE`, `README-controller.md`, `controller.example.toml`, and `videnoa-controller.exe`.

The scripts enforce platform binary shape, exact `videnoa-controller 0.1.2` version output, deterministic archive metadata/order, exact member allowlists, and GPU/runtime/secret exclusions. `crates/controller/Cargo.toml` has no `videnoa-core`, ORT, CUDA, cuDNN, TensorRT, or model dependency, and the fresh normal/build dependency-tree rejection check passed.

## Release Graph, Names, and Existing Product Preservation

The release workflow still defines independent Controller outputs:

- `controlnet/videnoa-controller:0.1.2` and `controlnet/videnoa-controller:latest`;
- `videnoa-controller-v0.1.2-linux-x86_64.tar.gz`;
- `videnoa-controller-v0.1.2-windows-x86_64.zip`.

Legacy outputs remain separate and unchanged:

- `controlnet/videnoa:0.1.2` and `controlnet/videnoa:latest`;
- `videnoa-linux64-0.1.2.7z*`;
- `videnoa-win64-0.1.2.7z*`;
- archive root `videnoa/` with the existing `videnoa`, `videnoa-desktop`, `lib`, `bin`, `models`, `presets`, `README.md`, and `LICENSE` layout.

The release version gate requires matching app, Controller, core, and desktop versions. The reusable quality gate enables packaging checks. GitHub Release creation depends on both legacy archives, both Controller archives, and both Docker publication jobs. Release verification checks all expected archive names and pulls version plus `latest` tags for both images.

Run `33919997302` is a `dev` push smoke workflow. It did not publish a GitHub Release, create a release tag, or push Docker Hub images. Actual publication remains gated to the release workflow on `master` or an eligible manual dispatch with `publish == true`. This audit makes no claim that production publication occurred.

## Migration, Backup, Restore, Upgrade, and Rollback

Controller startup applies the ordered SQLx migrations automatically in WAL mode. The fresh exact-tip migration test passed all three cases:

- fresh database migration and effective SQLite pragmas;
- migration-5 to current upgrade preserving settings and uniqueness contracts;
- invalid migration rollback without partial schema or false success recording.

Migration `0006_submission_ownership.sql` is additive. The operator guides require pausing and cleanly stopping Controller, preserving the full `data_root` including WAL/SHM files, configuration and protected hash, persistent temp/recovery state, NAS roots, and matching worker `jobs.db` plus workspaces. Upgrade applies migrations at startup and requires health, authenticated readiness, worker, retained-history, and reconciliation checks. Rollback requires restoring the complete pre-upgrade Controller and worker snapshots with the previous binary/image/configuration and explicitly forbids pointing an older binary at the migrated database. No unsupported downgrade command is claimed.

## Independent Local Verification

The following fresh exact-tip checks passed:

- positive and mutation-negative CI/release workflow contracts;
- direct workflow validation;
- deterministic Linux Controller archive, filename/version/layout, missing-file, wrong-version, and forbidden-content contracts;
- Windows Controller packaging static target/name/layout/determinism/version/GPU-isolation contracts;
- Controller archive root documentation/config completeness and secret-pattern checks;
- legacy split/single archive, missing-output, insufficient-space, and fatal-p7zip propagation contracts;
- Controller documentation contract checks;
- Controller container source contract;
- `cargo test --locked -p videnoa-controller --test persistence_migrations` (`3 passed; 0 failed`);
- Controller normal/build dependency isolation from `videnoa-core`, ORT, ONNX Runtime, CUDA, cuDNN, and TensorRT;
- protected-path range comparison and `git diff --check`.

The Linux Controller archive test uses a compiled test-fixture executable to exercise deterministic packaging and rejection behavior; production binary authority is the successful hosted Linux Controller archive job. Native Windows production authority is the successful hosted Windows job.

Secret Guard's tracked scan reported only the two known pre-existing fixture/redaction literals in `controller-web/src/api/workerSchemas.test.ts` and `crates/core/src/logging.rs`; neither file changed in `67620f5..HEAD`, and no evidence or package artifact was flagged. The repository still has the previously documented generic key-file `.gitignore` coverage gaps. No new durable notepad finding was added because this exact p7zip `E_FAIL` mode and the Secret Guard observations are already recorded.

## Rejection Basis

F4 requires every required job in the exact current-HEAD push run to pass. `Package smoke (Linux)` failed and did not verify the legacy Linux archive. Local fixture success, unchanged packaging topology, and the other 13 hosted successes do not satisfy that gate. The branch provenance and landing state are correct, Controller delivery contracts pass, and no GPU/runtime leakage or legacy naming/layout change was found, but exact-tip CI is not green.

VERDICT: REJECT
