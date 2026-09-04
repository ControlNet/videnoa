# F4 Packaging, Release, Regression, and Repository-Landing Audit

Audit date: 2026-09-04

Audited implementation tip: `094d87833ebbc55c8f27857cd70d85fc5cc91afe`

Prior rejected tip: `22f9656b797f5c3326ff003d11d812a6d212ed8b`

Hosted authority: [GitHub Actions run 33873102244](https://github.com/ControlNet/videnoa/actions/runs/33873102244)

## Decision

The exact clean pushed tip passes the complete hosted Controller and existing-product regression graph. The prior timing-sensitive Task 20 failure is superseded by two successful exact-tip executions: the aggregate Controller Rust job and the separately gated crash/outage suite both pass the repaired 30-test Task 20 target, including the three-worker real-HTTP scenario and submission-ownership cases.

Dedicated Controller image, Linux archive, and native Windows archive smoke jobs all pass. The release graph preserves the existing `videnoa` image, binary, and archive contracts while adding independent `videnoa-controller` outputs. Migration 0006 is additive and passes fresh, upgrade, idempotency, and transactional rollback tests. No F4 blocker remains.

## Exact-Tip Hosted Regression

Run `33873102244` completed with conclusion `success` for push event `dev` at exact head SHA `094d87833ebbc55c8f27857cd70d85fc5cc91afe`.

| Required job | Job ID | Result |
|---|---:|---|
| Workflow contracts | `101023548476` | PASS |
| Controller Rust quality and tests | `101023548983` | PASS |
| Controller web quality and E2E | `101023548784` | PASS |
| Controller fault and load suites | `101026168839` | PASS |
| Controller image and content smoke | `101026168906` | PASS |
| Controller archive smoke (Linux) | `101026169452` | PASS |
| Controller archive smoke (Windows) | `101026168857` | PASS |
| Rust tests (Ubuntu) | `101023548739` | PASS |
| Rust tests (Windows) | `101023548868` | PASS |
| Web build check (Ubuntu) | `101023548719` | PASS |
| Web build check (Windows) | `101023548786` | PASS |
| Package smoke (Linux) | `101024442401` | PASS |
| Package smoke (Windows) | `101024442350` | PASS |
| Docker build smoke | `101024442430` | PASS |

The Controller Rust job passed formatting, strict Clippy, and all Controller targets. Its Task 20 target reported `30 passed; 0 failed`, including:

- `three_worker_real_http_pipeline_uses_all_capacity_without_duplicates`
- `normal_attempt_submits_exactly_once`
- `same_key_replay_maps_to_one_remote_job`
- `same_generation_cancellation_defers_owned_submission_without_duplicate_request`
- `timed_out_submission_waits_for_restart_before_replay`

The separately gated fault/load job repeated Task 20 with `30 passed; 0 failed`, then passed Task 21 load, concurrency, filesystem, resource, and security targets with `7 + 18 + 22 + 1 + 46` tests and no failures. This directly supersedes the prior exact-SHA Task 20 failure rather than relying on a local-only rerun.

## Dedicated Controller Image

The hosted image job built `Dockerfile.controller` and ran `bash scripts/check_controller_container.sh videnoa-controller:ci --all`. Its log records:

- source contract: PASS
- image contract: PASS
- runtime health, embedded SPA, writable mounts, and restart persistence: PASS
- missing configuration, missing password hash, and unwritable data failures: PASS
- outside-root task rejection with HTTP 400 and an explicit configured-root error: PASS

The verified contract requires Debian bookworm slim, numeric non-root identity `10001:10001`, entrypoint `videnoa-controller`, the documented config/data/temp/input/output mounts, and the healthcheck. Runtime inspection rejects the legacy `/usr/local/bin/videnoa`, Node/npm, loose models, and installed or linked ONNX Runtime, CUDA, cuDNN, TensorRT, or NVIDIA content.

`crates/controller/Cargo.toml` has no `videnoa-core` or GPU/model-runtime dependency. The image and archive checks inspect both linkage and packaged content, so the Controller delivery remains GPU-free rather than merely avoiding GPU execution during smoke.

## Controller Archives

### Linux

The hosted Ubuntu 22.04 job built with Rust 1.83 through `scripts/package_controller.sh`, verified the archive, and produced:

`videnoa-controller-v0.1.2-linux-x86_64.tar.gz`

The hosted log records both `archive layout verified` and `archive created successfully`. The packaging contract enforces one exact versioned root containing only:

- `LICENSE`
- `README-controller.md`
- `controller.example.toml`
- `videnoa-controller`

The script validates an ELF x86-64 executable, exact `videnoa-controller 0.1.2` version output, and absence of linked ORT/CUDA/cuDNN/TensorRT libraries. It creates deterministic owner, mode, time, ordering, tar, and gzip metadata. A fresh exact-tip local `bash scripts/tests/package_controller_test.sh` rerun also passed deterministic equality, version, layout, missing-file, and forbidden-content cases.

### Windows

The hosted native Windows job built with Rust 1.83 through `scripts/package_controller.ps1`, verified the archive twice, and produced:

`videnoa-controller-v0.1.2-windows-x86_64.zip`

The hosted log records `archive layout verified` and `archive created successfully`. The PowerShell contract checks the PE signature, executes `--version` on Windows, rejects GPU/runtime DLL references, normalizes ZIP timestamps, and enforces one exact versioned root containing only `LICENSE`, `README-controller.md`, `controller.example.toml`, and `videnoa-controller.exe`. A fresh exact-tip local static contract rerun passed; native execution authority comes from the successful hosted Windows job.

## Release Graph and Existing Product Preservation

The workflow validator passed its complete positive matrix and every negative mutation, including omitted Controller assets/tags, removed GPU-content checks, version-gate damage, and bypass of the shared legacy Linux archive helper.

Controller outputs remain independent:

- `controlnet/videnoa-controller:0.1.2`
- `controlnet/videnoa-controller:latest`
- `videnoa-controller-v0.1.2-linux-x86_64.tar.gz`
- `videnoa-controller-v0.1.2-windows-x86_64.zip`

Existing outputs remain unchanged and separately verified:

- `controlnet/videnoa:0.1.2` and `controlnet/videnoa:latest`
- `videnoa-linux64-0.1.2.7z*`
- `videnoa-win64-0.1.2.7z*`
- existing `videnoa` binary and versioned archive root layout

The exact-tip hosted legacy Ubuntu/Windows Rust and web jobs, Linux/Windows package smoke jobs, and GPU Docker CLI smoke all passed. The release workflow gates publication on the reusable full quality workflow, requires both legacy and Controller archives/images before GitHub Release publication, and verifies all release asset names plus both products' version and latest image tags after publication.

## Migration, Backup, Restore, Upgrade, and Rollback

The audited range adds `0006_submission_ownership.sql`, a nullable `TEXT` ownership column on `task_attempts`. Exact-tip hosted tests and a fresh local `cargo test -p videnoa-controller --test persistence_migrations` run both passed all three migration cases:

- fresh database applies all six migrations and effective SQLite pragmas
- an existing migration-5 database upgrades idempotently to migration 6 while preserving settings and the worker/remote-job uniqueness index
- an invalid SQLx migration rolls back its partial schema and records no successful migration

The operator procedures match the implementation boundary:

- pause scheduling, reach a known state, stop cleanly, and copy the complete Controller `data_root`, including WAL/SHM files when present
- preserve configuration, password hash, NAS roots, Controller temp state, and matching worker `jobs.db` plus workspaces
- start the upgraded binary/image with the same state and let SQLx apply pending migrations atomically
- verify health, authenticated readiness, workers, retained task/attempt history, and nonterminal reconciliation before resuming
- roll back only by restoring the complete pre-upgrade Controller and matching worker snapshot; never run an older binary against a newer migrated database

This full-snapshot procedure covers the new submission ownership state without requiring or claiming an unsupported migration downgrade. Exact-tip crash/outage and submission-ownership suites also prove restart convergence, clean shutdown draining, and no duplicate remote compute across the new ownership boundary.

## Repository Landing and Audit Boundaries

At audit start and again before report editing:

- `HEAD == origin/dev == 094d87833ebbc55c8f27857cd70d85fc5cc91afe`
- branch divergence was `0 0`
- the original worktree was clean
- a detached audit worktree at the exact SHA was clean
- `git diff --check 22f9656..094d878` passed

The final five commits contain Task 20 test stabilization and notepad evidence only; they do not change packaging scripts, Dockerfiles, release artifact names, or release publication wiring. The successful hosted run checks the exact pushed result of those remediations.

GitHub's artifacts API reports `total_count: 0` for run `33873102244`, so no retained downloadable archive is claimed. Approval relies on the exact-tip hosted build/execute/verify logs and the workflow contracts, not on nonexistent retained artifacts. Real Docker Hub publication and GitHub Release publication were not triggered by this `dev` push and are not claimed as executed.

Node `punycode`, Node 20 action-runtime, npm, and ESLint deprecation notices are non-blocking warnings. They did not fail a required build, test, archive, image, or release-contract step.

VERDICT: APPROVE
