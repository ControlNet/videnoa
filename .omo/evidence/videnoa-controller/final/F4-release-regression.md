# F4 Packaging, Release, Regression, and Repository-Landing Audit

Audit date: 2026-09-05

Audited landing tip: `9bdeb4dc3d14a1e32253534dbd9d04c840dcb8c8`

Source fix: `30d9f25d19cf0ec1a88733483da7f95581e980ad` (`fix(controller): prevent auth executor starvation`)

Evidence landing commit: `9bdeb4dc3d14a1e32253534dbd9d04c840dcb8c8` (`docs(controller): record final review gate state`)

Exact-tip hosted authority: [GitHub Actions run 33949543240](https://github.com/ControlNet/videnoa/actions/runs/33949543240)

## Decision

**APPROVE**

The final F4 gate is satisfied. The repository was clean and exactly aligned with the live `dev` branch at audit start, the source remediation is the direct parent of the evidence landing commit, committed F1-F3 reports all approve, and the literal landing tip has a completed successful push run with exactly 14 successful jobs.

The three prior F4 blockers are closed: legacy Linux packaging succeeds with the resource-bounded helper, the formerly failing concurrent duplicate-intake regression passes in the hosted fault/load job on the remediated source tree, and the previously unlanded F1-F3 evidence state is committed and pushed. This is release-readiness and smoke evidence only; no production release, tag, archive upload, or container publication is claimed.

## Repository Provenance and Audit-Start Landing State

Before this reviewer modified any file, read-only Git inspection established:

- Current branch: `dev`.
- `HEAD`: `9bdeb4dc3d14a1e32253534dbd9d04c840dcb8c8`.
- Local tracking ref `origin/dev`: `9bdeb4dc3d14a1e32253534dbd9d04c840dcb8c8`.
- Live remote `refs/heads/dev` from `git ls-remote`: `9bdeb4dc3d14a1e32253534dbd9d04c840dcb8c8`.
- `HEAD...origin/dev`: `0 0`.
- Audit-start worktree: clean; `git status --porcelain=v1 --branch` reported only `## dev...origin/dev`.

Commit ancestry and content are unambiguous:

- `9bdeb4d` has parent `30d9f25`.
- `30d9f25` changes nine Controller authentication/error/test paths and contains the executor-starvation remediation plus its deterministic concurrent-intake regression.
- `9bdeb4d` changes only eight `.omo` review/evidence/plan paths. It changes no product source, test, workflow, package script, manifest, lockfile, Dockerfile, or release implementation.
- `crates/controller` has the same tree at `30d9f25` and `9bdeb4d`: `0eb1068692b827c107ac5ba51692d38bed5ba03b`.
- `controller-web` has the same tree at `30d9f25` and `9bdeb4d`: `1e21aa9ac5a1546ecafacf50473ef1c10afed070`.

The F1, F2, and F3 reports are tracked in `9bdeb4d`, their latest commit is `9bdeb4d`, and each ends with `VERDICT: APPROVE`. F3 approves English and Chinese for the initial release and explicitly keeps Korean out of scope; this F4 decision does not add or imply a Korean-support requirement.

Writing this F4 report and appending the required notepad entry necessarily makes those reviewer-owned files differ from `HEAD` after the audit. That post-audit reviewer output does not alter or contradict the independently observed clean audit-start state. No commit, push, pull, fetch, reset, restore, stash, clean, checkout, workflow rerun, release creation, or registry mutation was performed.

## Exact-Tip Hosted CI Authority

GitHub CLI and the read-only Actions API independently report run `33949543240` as:

- workflow: `Unittest Workflow`;
- event: `push`;
- branch: `dev`;
- head SHA: exactly `9bdeb4dc3d14a1e32253534dbd9d04c840dcb8c8`;
- status/conclusion: `completed/success`;
- jobs API count: exactly `14`.

All 14 jobs are `completed/success` at the same head SHA:

| Required job | Job ID | Result |
|---|---:|---|
| Web build check (windows-latest) | `101261586290` | PASS |
| Rust tests (ubuntu-latest) | `101261586363` | PASS |
| Rust tests (windows-latest) | `101261586378` | PASS |
| Controller web quality and E2E | `101261586380` | PASS |
| Web build check (ubuntu-latest) | `101261586416` | PASS |
| Workflow contracts | `101261586418` | PASS |
| Controller Rust quality and tests | `101261586450` | PASS |
| Docker build smoke | `101262050293` | PASS |
| Package smoke (Windows) | `101262050298` | PASS |
| Package smoke (Linux) | `101262050301` | PASS |
| Controller fault and load suites | `101262828877` | PASS |
| Controller archive smoke (Windows) | `101262828971` | PASS |
| Controller image and content smoke | `101262828982` | PASS |
| Controller archive smoke (Linux) | `101262829020` | PASS |

The fault/load log directly records:

- crash/outage suite: `31 passed; 0 failed`;
- load suite: `7 passed; 0 failed`;
- concurrency suite: `20 passed; 0 failed`;
- filesystem suite: `22 passed; 0 failed`;
- resource suite: `1 passed; 0 failed`;
- security suite: `47 passed; 0 failed`.

Most importantly, the hosted log explicitly shows `task_api::concurrency::concurrent_duplicate_intake_creates_exactly_one_task ... ok`. Because `9bdeb4d` directly contains parent `30d9f25` and changes only `.omo` files above it, this is exact-landing-tip hosted coverage of the remediated source tree. The prior HTTP 500 signal is closed rather than bypassed or attributed to infrastructure.

## Packaging and Image Acceptance

### Fresh local contract confirmation

The following focused commands were run fresh against the clean landing tip and passed:

```bash
bash scripts/tests/package_dist_archive_test.sh
bash scripts/tests/package_controller_test.sh
bash scripts/tests/package_controller_windows_static_test.sh
bash scripts/tests/controller_archive_root_files_test.sh
node --test scripts/tests/validate_ci_release_workflows.test.mjs
```

These checks confirm split/unsplit legacy archive handling, bounded p7zip invocation, missing-output and fatal-error propagation, deterministic Controller Linux packaging, static Windows packaging contracts, required root documents, and the complete positive and negative CI/release workflow matrix.

### Legacy products

The exact-tip hosted jobs confirm both legacy package smokes and the existing Docker smoke:

- Linux builds the legacy bundle, checks runtime compatibility, reclaims build cache, creates the 2000 MiB split archive through `scripts/package_dist_archive.sh`, and verifies the archive through `.7z.001` or the unsplit fallback.
- Windows builds the legacy bundle and verifies the split archive contains the unchanged `videnoa/` root.
- Existing image names remain `controlnet/videnoa:<version>` and `controlnet/videnoa:latest`.
- Existing archive names remain `videnoa-linux64-<version>.7z*` and `videnoa-win64-<version>.7z*`.

### Controller archives

The exact Controller release names are:

- `videnoa-controller-v0.1.2-linux-x86_64.tar.gz`;
- `videnoa-controller-v0.1.2-windows-x86_64.zip`.

Each archive has one versioned root containing only `LICENSE`, `README-controller.md`, `controller.example.toml`, and the platform Controller binary. The scripts reject wrong names, versions, member order/layout, missing files, loose frontend assets, models, caches, keys/certificates, and ONNX Runtime/CUDA/cuDNN/TensorRT runtime content. The hosted Linux job builds and verifies the archive on Ubuntu 22.04; the hosted Windows job builds and verifies it natively on Windows.

### Controller image

The Controller image contract remains independent at:

- `controlnet/videnoa-controller:0.1.2`;
- `controlnet/videnoa-controller:latest`.

`Dockerfile.controller` uses an isolated frontend stage, Rust 1.83 builder, and `debian:bookworm-slim` runtime. The runtime entrypoint is `videnoa-controller`, runs as numeric user/group `10001:10001`, exposes the healthcheck and persistent/NAS mounts, and contains no legacy `videnoa` binary, model payload, Node/npm, ONNX Runtime, CUDA, cuDNN, TensorRT, NVIDIA package, or runtime cache. Run `33949543240` passed the full `scripts/check_controller_container.sh ... --all` image/content job.

## Release Workflow and Publication Boundary

The fresh workflow validator passed the complete release graph and every negative mutation. The release version gate compares app, Controller, core, and desktop versions. GitHub Release creation depends on the quality gate, both legacy packages, both Controller packages, and both legacy/Controller image publication jobs. Final verification requires both products' version and `latest` image tags plus all four archive products.

Run `33949543240` is a `dev` smoke/quality workflow, not the `master` release workflow. Its artifacts API reports `total_count: 0`. This audit did not create a release tag or GitHub Release, upload archives, or publish/pull Docker Hub images. Approval means the packaging and release contracts are ready and regression-tested, not that a production release has occurred.

## Regression, Evidence, and Security Disposition

- Existing Videnoa Rust tests pass on Linux and Windows.
- Existing Web production builds pass on Linux and Windows.
- Controller Rust formatting, strict Clippy, and tests pass under Rust 1.83.
- Controller Web lint, unit tests, production build, and Chromium E2E pass.
- Controller crash/outage and load/concurrency/filesystem/resource/security suites pass.
- Both legacy packages, the legacy Docker image, both Controller archives, and the Controller image/content checks pass.
- Migration and rollback evidence was inspected concretely: `.omo/evidence/videnoa-controller/task-3/atomic-failures.txt` records `12 passed, 0 failed`, including an invalid SQLx migration rolling back preceding DDL without a false successful migration record; `.omo/evidence/videnoa-controller/final/remediation-f4-duplicate-run/verification.md` records a real database stopped at migration `0005` upgrading through `0006` while preserving the prior unique index and data contract.
- Backup/restore and upgrade/rollback instructions were inspected in `docs/controller.md` and `README-controller.md`. `.omo/evidence/videnoa-controller/task-25/docs-smoke.txt` records the documentation contract, example-config load, isolated startup, health/readiness, persisted SQLite, and clean shutdown passing; `scripts/tests/controller_docs_test.sh` requires the Backup/Restore and Upgrade/Rollback sections, durable `controller.sqlite3`/`jobs.db` evidence, exact artifact names, valid links, and no plaintext secret assignment.
- No new destructive production migration, backup/restore drill, downgrade, or rollback was executed or claimed by this read-only audit. The exact-tip `Controller Rust quality and tests` job is the current source regression authority for the committed migration and documentation tests.

Secret Guard's tracked scan reported two known synthetic test literals: an intentionally rejected unknown `secret` field in a frontend schema test and a split-write `token=` redaction fixture in core logging tests. Direct inspection confirms neither is a real credential. The `.gitignore` audit retains 19 pre-existing generic sensitive-file coverage gaps; no tracked real secret or F4 evidence leak was found, and this audit does not modify `.gitignore`.

## Prior Blocker Closure

| Prior blocker | Final disposition |
|---|---|
| Legacy Linux p7zip/package failure | CLOSED: resource-bounded helper contracts pass locally and exact-tip Linux package smoke passes. |
| Concurrent duplicate-intake HTTP 500 | CLOSED: `30d9f25` moves password-file/Argon2 work to `spawn_blocking`, and the exact-tip fault/load log shows the formerly failing test and its suite passing. |
| Dirty, uncommitted, or unpushed final evidence | CLOSED: audit started clean with `HEAD == origin/dev == live remote dev == 9bdeb4d` and divergence `0 0`; F1-F3 and the prior gate records are committed. |

## Remaining Blockers

None.

VERDICT: APPROVE
