VERDICT: REJECT

# F4 Packaging, Release, Regression, and Repository-Landing Audit

Audit date: 2026-09-04

Audited implementation tip: `22f9656b797f5c3326ff003d11d812a6d212ed8b`

Prior rejected baseline: `b06567b6d9cc3ad0466dd56f04e2a3e5d60f6144`

The rerun resolves the prior strict-Clippy, Controller-web, and legacy Linux archive failures, but the exact-SHA hosted Controller Rust job fails a required real-HTTP multi-worker pipeline test. Local reruns do not supersede a failed required hosted check without concrete evidence that the hosted failure was infrastructure-only. The candidate therefore does not satisfy F4's complete release-success and regression requirements.

## Blocking Finding

### F4-B1: Exact-SHA hosted Controller test suite fails

- GitHub Actions run: https://github.com/ControlNet/videnoa/actions/runs/33827264003
- Required job: `Controller Rust quality and tests`, job `100882475704`: https://github.com/ControlNet/videnoa/actions/runs/33827264003/job/100882475704
- Checkout, Rust setup, Node setup, formatting, and strict Controller Clippy all passed.
- `Run Controller tests` failed with exit code 101.
- Failing test: `multi_worker::three_worker_real_http_pipeline_uses_all_capacity_without_duplicates` in the `task20` integration target.
- Assertion: `crates/controller/tests/task20/support/proof.rs:81`, where the mock observed two `Run` requests instead of the expected one (`left: 2`, `right: 1`).
- Target result: `25 passed; 1 failed`.
- Downloaded log: `/tmp/opencode/f4-controller-rust-job-current.log`, SHA-256 `f69ab1bec126e9d7033c047fe375c67ad961a9b4f86f218d7a7e5a8082631f9d`.
- The same scenario passed twice locally in isolation and the full local Controller suite passed, so the defect may be timing-sensitive. That is not proof of an infrastructure-only failure; it remains an unsuperseded product test failure in a required clean hosted run.
- Dependent hosted `Controller fault and load suites`, Controller Linux/Windows archive jobs, and `Controller image and content smoke` were skipped because their prerequisites did not all succeed.

Any one required regression failure is sufficient to reject F4. The remaining sections record supporting passes and boundaries; they do not override F4-B1.

## Dedicated Controller Image

- Built exact-tip `Dockerfile.controller` as `videnoa-controller:f4-22f9656`.
- Image ID: `sha256:3bca0d6e26d64c4c1f248b943c32a19634534aabd328de01695c6a3bcfa22471`.
- Size: `104598352` bytes.
- Runtime identity: numeric non-root `10001:10001`.
- Entrypoint: `["videnoa-controller"]`.
- `bash scripts/check_controller_container.sh videnoa-controller:f4-22f9656 --all`: PASS for source and metadata contracts, health, embedded SPA, SQLite persistence, and negative configuration, hash, permission, and root-path cases.
- Exported root filesystem: `/tmp/opencode/f4-22f9656-controller-rootfs.tar`, SHA-256 `e84fb06ce54fd2011c279dd7f287dc166400548028906c6b4587d90b3f9e457b`.
- Image and exported-filesystem inspection found no CUDA, cuDNN, TensorRT, ONNX Runtime, NVIDIA, model, cache, Node/npm, loose Controller web source, or legacy `/usr/local/bin/videnoa` content.
- Controller linkage contains only the ELF loader, `libgcc_s`, `libm`, and `libc`.
- Exact-SHA hosted legacy `Docker build smoke` passed.

## Controller Archives

### Linux

- Built through `scripts/package_controller.sh`, then independently repackaged the same binary and required root files.
- Artifacts:
  - `/tmp/opencode/f4-22f9656-controller-a/videnoa-controller-v0.1.2-linux-x86_64.tar.gz`
  - `/tmp/opencode/f4-22f9656-controller-b/videnoa-controller-v0.1.2-linux-x86_64.tar.gz`
- Both archives have SHA-256 `749ae1f9c5aa88dd059bbe00d6f0312392f518e9295c30beeeddbb380370734d`.
- Each archive has one exact versioned root and exactly four files: `LICENSE`, `README-controller.md`, `controller.example.toml`, and `videnoa-controller`.
- The extracted binary is ELF x86-64, has standard-library-only dynamic linkage, and passes `--version` and `--help`.
- Standalone smoke from `/tmp/opencode/f4-22f9656-controller-smoke` returned `{"status":"ok"}`, served the embedded SPA, persisted SQLite state with five migrations, and exited cleanly on SIGTERM. No loose `controller-web`, `dist`, or `assets` directory was required.
- `bash scripts/tests/package_controller_test.sh`: PASS.
- `bash scripts/tests/controller_archive_root_files_test.sh`: PASS.

### Windows Boundary

- No native `pwsh` or `powershell` executable is installed on this Linux host, so no local PE build or execution is claimed.
- `bash scripts/tests/package_controller_windows_static_test.sh`: PASS for exact naming, root files, version, forbidden content, and hosted execution contracts.
- The exact-SHA hosted Controller Windows archive job was skipped after prerequisite failure. Native Windows package proof therefore remains unavailable for this rejected run.

## Release Graph and Legacy Packaging

- `bash scripts/tests/package_dist_archive_test.sh`: PASS for split and single archives, missing verification output, successful creation without output rejection, insufficient disk, and fatal `7z` propagation.
- `.github/workflows/unittest.yaml` and `.github/workflows/release.yaml` both route legacy Linux archive creation and verification through `scripts/package_dist_archive.sh`.
- `node --test scripts/tests/validate_ci_release_workflows.test.mjs`: PASS for the positive CI/release graph and all negative mutations, including direct helper-bypass mutations.
- Controller release names remain independent: `controlnet/videnoa-controller:<version>`, `controlnet/videnoa-controller:latest`, `videnoa-controller-v<version>-linux-x86_64.tar.gz`, and `videnoa-controller-v<version>-windows-x86_64.zip`.
- Existing `videnoa` image tags, binary names, and archive names remain separate.
- A real multi-gigabyte local legacy package was not represented as complete. The helper fixture suite, workflow contracts, and hosted package jobs are the applicable evidence.
- Exact-SHA hosted legacy `Package smoke (Linux)` and `Package smoke (Windows)` both completed successfully.

## Existing Product Regression Results

- `cargo +1.83.0 clippy -p videnoa-controller --all-targets --all-features -- -D warnings`: PASS.
- `cargo clippy -p videnoa-controller --all-targets --all-features -- -D warnings`: PASS.
- `cargo +1.83.0 test -p videnoa-controller --all-targets`: PASS locally, including the hosted failing scenario when rerun twice in isolation.
- Controller web lint, Vitest, and production build passed locally and in the exact-SHA hosted `Controller web quality and E2E` job.
- Legacy `web` lint, Vitest, and production build passed locally; exact-SHA hosted Linux and Windows web build checks passed.
- Existing legacy Rust tests passed locally with the documented conda `anime`, ONNX Runtime, TensorRT, library, and pkg-config environment. Exact-SHA hosted legacy Rust tests passed on Ubuntu and Windows; the separate Controller Rust job failed as documented above.
- Built legacy image `videnoa:f4-22f9656`; `docker run --rm videnoa:f4-22f9656 videnoa --help` passed. Image ID `sha256:07cafb9514f29b3c44ff7eb77a3ab67f7358d8f65fce5c82b5e04f55fd625d3e`, size `5806616206` bytes, command `["videnoa"]`.
- `cargo pkgid` reports `videnoa-app@0.1.2` and `videnoa-controller@0.1.2`; the Controller binary reports `videnoa-controller 0.1.2`.

## Migration, Backup, and Rollback

- Controller migration files and the documented backup/rollback procedure did not change between `b06567b` and `22f9656`, but Controller runtime code did. The prior F4 rehearsal is therefore historical supporting evidence, not an exact-tip migration pass.
- That rehearsal used isolated `/tmp/opencode/f4-recovery` state, started from a valid migration-1 database, ran migrations 2 through 5, verified health and retained worker state, stopped cleanly, and restored complete Controller and worker snapshots.
- Pre-backup and post-restore SHA-256 values matched for the Controller database, config, password-hash file, worker database, and worker workspace artifact.
- Restored state contained only migration 1, lacked later task columns, retained the original worker, and retained the mock processing job. No WAL/SHM files remained after clean stop.
- No exact-tip repeat of the complete migration/rollback rehearsal was performed in this rerun. The historical rehearsal validates the unchanged schema and stopped-state snapshot procedure, but it does not supersede the hosted regression failure or independently prove the changed runtime at `22f9656`.
- The rehearsal did not claim authenticated readiness, browser login, or nonterminal-task reconciliation from a synthetic fixture without known credentials or task records.

## Repository Landing and Security

- At exact-SHA audit start: `HEAD == origin/dev == @{upstream} == 22f9656b797f5c3326ff003d11d812a6d212ed8b`, both relevant divergences were `0 0`, the worktree was clean, and the stash was empty.
- The prior rejected baseline is an ancestor of the audited tip, with exactly 35 remediation commits in the audited range.
- Later commits at current `dev` are final-review evidence changes and are not part of the audited implementation SHA.
- `git diff --check b06567b..22f9656`: PASS.
- Repository-wide tracked secret scan reviewed 639 files and found only two test fixtures, not credentials: a strict-schema rejection value named `must-not-cross` and a split logging-redaction input assembled from a token label plus `abc123`.
- No actual key, token, password, private key, or credential file was identified.
- `.gitignore` covers `.env`; the secret scanner reports 19 uncovered sensitive filename patterns. This is a hardening observation, not evidence that such files are tracked.

## Hosted and Environmental Boundaries

- The exact-SHA GitHub Actions run completed with conclusion `failure`: all independent workflow-contract, Controller-web, legacy Rust/web, legacy Docker, and legacy Linux/Windows package jobs passed; the Controller Rust job failed and its dependent Controller fault, archive, and image jobs were skipped.
- Native Windows Controller execution and archive creation were not available locally.
- Docker Hub publication and GitHub Release publication were not triggered. Workflow contracts were inspected read-only; no real push, release asset upload/download, or published-tag pull is claimed.
- F4 rejection depends only on the completed required hosted test failure, not on pending jobs or unavailable publication side effects.
