# F4 Packaging, Release, Regression, and Repository-Landing Audit

Audit date: 2026-09-05

Audited implementation tip: `67620f5b45d54205b44861e3cc5d59e88724e998`

Comparison baseline only: `094d87833ebbc55c8f27857cd70d85fc5cc91afe`

Hosted authority: [GitHub Actions run 33910972161](https://github.com/ControlNet/videnoa/actions/runs/33910972161)

## Decision

The exact pushed tip passes the complete hosted regression and packaging-smoke graph. This decision was made independently from the prior `094d878` approval: run `33910972161` is a completed `success` push run on `dev` whose head SHA exactly equals `67620f5b45d54205b44861e3cc5d59e88724e998`, and every one of its 14 jobs succeeded.

The remediation range changes Controller UI behavior, authentication throttling, tests, evidence, and three patch-level lockfile resolutions. It does not change release workflows, Dockerfiles, packaging scripts, artifact names, or publication wiring. Exact-tip Controller, legacy Videnoa, archive, image, Windows, Linux, browser, fault, load, and security checks all pass. No F4 blocker remains.

## Exact-Tip Hosted Regression

Run `33910972161` completed with conclusion `success` for push event `dev` at exact head SHA `67620f5b45d54205b44861e3cc5d59e88724e998`.

| Required job | Job ID | Result |
|---|---:|---|
| Workflow contracts | `101147034790` | PASS |
| Rust tests (Ubuntu) | `101147034911` | PASS |
| Rust tests (Windows) | `101147034916` | PASS |
| Web build check (Ubuntu) | `101147035017` | PASS |
| Controller web quality and E2E | `101147035048` | PASS |
| Controller Rust quality and tests | `101147035062` | PASS |
| Web build check (Windows) | `101147035080` | PASS |
| Docker build smoke | `101148113738` | PASS |
| Package smoke (Linux) | `101148113756` | PASS |
| Package smoke (Windows) | `101148113781` | PASS |
| Controller image and content smoke | `101149593261` | PASS |
| Controller archive smoke (Windows) | `101149593325` | PASS |
| Controller archive smoke (Linux) | `101149593387` | PASS |
| Controller fault and load suites | `101149593732` | PASS |

The Controller Rust job passed frontend embedding, formatting, strict Clippy, and all Controller test targets. The separately gated fault/load job passed the crash/outage target and the load, concurrency, filesystem, resource, and security targets. The Controller web job passed lint, unit tests, production build, Chromium installation, and browser E2E tests. Both legacy Rust and web matrices passed on Ubuntu and Windows.

## Remediation Range and Regression Risk

The five remediation commits after the comparison baseline are:

- `52b7b5991b5354f7d52214a51ec3d9951527b4fd`: frontend table navigation.
- `b00a3c12a06eaa846b1934a120c0777fffc68cd8`: recoverable error focus.
- `d4859fd7ef8734cd93f897e6da083c65b241f7ea`: Bearer-auth throttling.
- `e1be5fa5f4fe3372f6c9c17592eeec0b236e2e93`: lockfile security patches.
- `67620f5b45d54205b44861e3cc5d59e88724e998`: final evidence and notepad updates.

The aggregate `094d878..67620f5b` diff contains no change under `.github/workflows`, no Dockerfile change, and no change to the legacy or Controller packaging scripts. `git diff --check` passes for the range. `Cargo.lock` changes only `anyhow 1.0.101 -> 1.0.103`, `h2 0.4.13 -> 0.4.16`, and `rustls-webpki 0.103.9 -> 0.103.13`; exact-tip hosted Rust, Controller, archive, image, and legacy jobs exercise those resolutions.

## Dedicated Controller Image

The hosted image job built `Dockerfile.controller` and successfully ran the full image/content verification. The contract requires:

- isolated Node frontend and Rust 1.83 builder stages;
- Debian bookworm-slim runtime;
- numeric non-root identity `10001:10001`;
- `videnoa-controller` entrypoint and documented config/data/temp/input/output mounts;
- embedded SPA, liveness healthcheck, writable persistent mounts, and restart persistence;
- explicit startup failures for missing config, missing password hash, and unwritable data;
- configured-root rejection for an outside-root task;
- no legacy `videnoa` binary, Node/npm, models, ONNX Runtime, CUDA, cuDNN, TensorRT, or NVIDIA runtime content.

`crates/controller/Cargo.toml` has no dependency on `videnoa-core`, `ort`, or a GPU/model runtime. A fresh local normal-dependency tree check also found none of those crates.

## Controller Archives

The successful hosted Linux archive job used Rust 1.83 and `scripts/package_controller.sh` to build and verify `videnoa-controller-v0.1.2-linux-x86_64.tar.gz`. The script enforces an ELF x86-64 executable, exact `videnoa-controller 0.1.2` output, no forbidden GPU/runtime linkage, deterministic metadata, and one versioned root containing only `LICENSE`, `README-controller.md`, `controller.example.toml`, and `videnoa-controller`.

The successful hosted native Windows job used Rust 1.83 and `scripts/package_controller.ps1` to build and verify `videnoa-controller-v0.1.2-windows-x86_64.zip`. The script enforces a PE executable, native `--version` proof, deterministic ZIP timestamps and ordering, no forbidden GPU/runtime DLL reference, and the same exact root files with `videnoa-controller.exe`.

Fresh local checks passed deterministic Linux archive equality, filename/version/layout gates, missing-file failure, forbidden-content failure, Windows packaging static contracts, and archive root-file completeness. The Linux local archive test intentionally uses a compiled test-fixture executable to exercise packaging determinism and rejection behavior; production binary authority remains the successful hosted Linux and native Windows jobs.

## Release Graph and Existing Product Preservation

The release graph keeps Controller outputs independent:

- `controlnet/videnoa-controller:0.1.2` and `controlnet/videnoa-controller:latest`;
- `videnoa-controller-v0.1.2-linux-x86_64.tar.gz`;
- `videnoa-controller-v0.1.2-windows-x86_64.zip`.

Existing outputs remain separate and unchanged:

- `controlnet/videnoa:0.1.2` and `controlnet/videnoa:latest`;
- `videnoa-linux64-0.1.2.7z*`;
- `videnoa-win64-0.1.2.7z*`;
- the existing `videnoa` binary and archive-root layout.

Publication is gated by a common version check across all crates and the reusable full quality workflow with packaging checks enabled. GitHub Release creation requires both legacy archives, both Controller archives, and both Docker publication jobs. The subsequent verification job requires the GitHub Release and both image publication jobs, checks all expected release asset names, and pulls both products' version and `latest` image tags.

The exact-tip local workflow validator passed the positive graph and all mutation-negative cases, including removal of a legacy package job, Controller Dockerfile use, embedded frontend build, Controller version gate, Controller release archive, Controller version image tag, GPU-content check, and legacy Linux archive helper use.

## Migration, Backup, Restore, Upgrade, and Rollback

The Controller has six ordered SQLx migrations. Migration `0006_submission_ownership.sql` additively introduces nullable `task_attempts.submission_owner`; it does not remove or rewrite persisted data. A fresh exact-tip local `cargo test --locked -p videnoa-controller --test persistence_migrations` run passed all three cases:

- fresh database applies all migrations and effective SQLite pragmas;
- an existing migration-5 database upgrades idempotently while preserving settings and worker/remote-job uniqueness;
- an invalid migration transaction rolls back partial schema and records no success.

The operator procedures match the implementation boundary: pause scheduling, reach known task states, stop cleanly, and preserve the full Controller `data_root`, WAL/SHM files when present, configuration, protected password hash, temp state, NAS roots, and matching worker `jobs.db` plus workspaces. Upgrade applies pending migrations at startup and requires health, authenticated readiness, worker, retained-history, and reconciliation checks before resume.

Rollback correctly requires the complete pre-upgrade Controller and matching worker snapshots plus the previous binary/image and configuration. It explicitly forbids running an older binary on the migrated database and makes no unsupported claim of migration downgrade support.

## Independent Local Verification

The following exact-tip checks passed:

- `node scripts/tests/validate_ci_release_workflows.test.mjs`;
- `node scripts/validate_ci_release_workflows.mjs`;
- `bash scripts/tests/package_controller_test.sh`;
- `bash scripts/tests/package_controller_windows_static_test.sh`;
- `bash scripts/tests/controller_archive_root_files_test.sh`;
- `cargo test --locked -p videnoa-controller --test persistence_migrations` (`3 passed; 0 failed`);
- normal dependency-tree rejection of `videnoa-core`, ORT, ONNX Runtime, CUDA, cuDNN, and TensorRT crates;
- `VIDENOA_CONTROLLER_WEB_PREBUILT=1 cargo +1.83 check --locked -p videnoa-controller` with `rustc 1.83.0`.

## Repository Landing and Audit Boundaries

Before report replacement:

- `HEAD == origin/dev == 67620f5b45d54205b44861e3cc5d59e88724e998`;
- branch divergence was `0 0`;
- the worktree was clean;
- `git diff --check 094d878..67620f5b` passed.

GitHub's artifacts API reports `total_count: 0` for run `33910972161`. No retained downloadable archive is claimed. Approval relies on exact-tip hosted build, execution, packaging, and verification jobs plus independent static/local contract checks.

The `dev` push did not execute real GitHub Release or Docker Hub publication, and this audit performed no registry write, tag, release, workflow, issue, commit, or push. Publication approval is therefore limited to the validated workflow graph and successful non-publishing package/image smokes, not a claim that production publication was exercised.

VERDICT: APPROVE
