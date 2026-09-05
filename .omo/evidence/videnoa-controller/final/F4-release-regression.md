# F4 Packaging, Release, Regression, and Repository-Landing Audit

Audit date: 2026-09-05

Current repository tip: `e161e772744c48791f67cb21575c6ebef4ace13c`

Last product revision: `0fb4eb597acda9b571efc686c4701da333831675`

Current exact-tip hosted authority: [GitHub Actions run 33940048214](https://github.com/ControlNet/videnoa/actions/runs/33940048214)

Last all-green product authority: [GitHub Actions run 33933923401](https://github.com/ControlNet/videnoa/actions/runs/33933923401)

## Decision

The former legacy Linux packaging rejection is closed. The resource-bounded archive helper is present, its focused local contract passes, and both the remediated product run `33933923401` and the current-tip run `33940048214` successfully created and verified the legacy Linux split archive. The current-tip legacy Windows package, legacy Docker image, Controller Linux archive, Controller Windows archive, and Controller image/content jobs also passed.

F4 nevertheless rejects the repository in its current landing state for two independent reasons:

1. The literal current-tip `push` run `33940048214` is `failure`, with 13 of 14 jobs successful. `Controller fault and load suites` failed because `task_api::concurrency::concurrent_duplicate_intake_creates_exactly_one_task` received an unexpected `500 Internal Server Error`.
2. The worktree was already dirty at audit start with uncommitted F1, F2, F3, knowledge, notepad, and plan edits. This audit adds the F4 report and appends the notepad as requested, but does not and cannot claim those results are committed or pushed.

The product tree outside `.omo/` is byte-for-byte identical between `0fb4eb5` and `e161e77`, and the current worktree has no product-source modification. That identity makes run `33933923401` valid evidence for the unchanged product revision, packaging remediation, and release contracts. It does not turn a successful run on `0fb4eb5` into a literal exact-tip successful run on `e161e77`, and it does not satisfy the plan's explicit clean-landing requirement.

## Current Repository Provenance and Landing State

Read-only Git inspection established:

- `HEAD`: `e161e772744c48791f67cb21575c6ebef4ace13c`.
- Local tracking ref `origin/dev`: `e161e772744c48791f67cb21575c6ebef4ace13c`.
- Actual remote `refs/heads/dev` from `git ls-remote`: `e161e772744c48791f67cb21575c6ebef4ace13c`.
- `HEAD...origin/dev`: `0 0`; the committed branch is up to date with the remote.
- Audit-start worktree: dirty, with six modified tracked paths:
  - `.omo/evidence/videnoa-controller/final/F1-plan-compliance.md`;
  - `.omo/evidence/videnoa-controller/final/F2-quality-security.md`;
  - `.omo/evidence/videnoa-controller/final/F3-manual-visual.md`;
  - `.omo/knowledges/videnoa-controller-f3-manual-visual.md`;
  - `.omo/notepads/videnoa-controller/learnings.md`;
  - `.omo/plans/videnoa-controller.md`.
- No source, workflow, lockfile, manifest, packaging script, Dockerfile, or other product path was modified in the worktree.
- No pull, push, commit, reset, restore, stash, clean, checkout, branch/remote change, workflow rerun, release creation, or registry write was performed.

The branch tip is pushed and non-divergent, but the repository is not landed because required final evidence and plan state remain uncommitted.

## Hosted CI Authority

### Run 33933923401 genuinely passed all 14 jobs

GitHub CLI and the read-only Actions API independently report run `33933923401` as `completed/success`, event `push`, branch `dev`, head SHA exactly `0fb4eb597acda9b571efc686c4701da333831675`. The jobs API reports `total_count: 14`, and every job conclusion is `success`:

| Required job | Job ID | Result |
|---|---:|---|
| Rust tests (ubuntu-latest) | `101218025130` | PASS |
| Controller web quality and E2E | `101218025156` | PASS |
| Workflow contracts | `101218025195` | PASS |
| Controller Rust quality and tests | `101218025206` | PASS |
| Web build check (ubuntu-latest) | `101218025211` | PASS |
| Web build check (windows-latest) | `101218025229` | PASS |
| Rust tests (windows-latest) | `101218025284` | PASS |
| Docker build smoke | `101218559007` | PASS |
| Package smoke (Windows) | `101218559010` | PASS |
| Package smoke (Linux) | `101218559087` | PASS |
| Controller image and content smoke | `101219127700` | PASS |
| Controller fault and load suites | `101219127705` | PASS |
| Controller archive smoke (Linux) | `101219127760` | PASS |
| Controller archive smoke (Windows) | `101219127763` | PASS |

This is the first verified hosted run after the legacy Linux p7zip resource fix and is valid all-green product-revision evidence.

### Run 33940048214 is the literal current-tip authority

Run `33940048214` is `completed/failure`, event `push`, branch `dev`, head SHA exactly `e161e772744c48791f67cb21575c6ebef4ace13c`. Its jobs API reports 14 jobs: 13 `success` and one `failure`.

| Current-tip job | Job ID | Result |
|---|---:|---|
| Controller web quality and E2E | `101235547281` | PASS |
| Web build check (ubuntu-latest) | `101235547307` | PASS |
| Web build check (windows-latest) | `101235547308` | PASS |
| Workflow contracts | `101235547321` | PASS |
| Controller Rust quality and tests | `101235547342` | PASS |
| Rust tests (ubuntu-latest) | `101235547346` | PASS |
| Rust tests (windows-latest) | `101235547393` | PASS |
| Package smoke (Linux) | `101236045217` | PASS |
| Package smoke (Windows) | `101236045261` | PASS |
| Docker build smoke | `101236045281` | PASS |
| Controller archive smoke (Windows) | `101236772909` | PASS |
| Controller archive smoke (Linux) | `101236772949` | PASS |
| Controller image and content smoke | `101236772952` | PASS |
| Controller fault and load suites | `101236772981` | **FAIL** |

The failed job first passed the complete crash/outage suite. In the load/concurrency/filesystem/resource/security step, the seven Task 21 load tests passed, including 20,000-row index/bounds coverage and the repeated mixed-intake race. Nineteen of twenty `task21_concurrency` tests then passed. `task_api::concurrency::concurrent_duplicate_intake_creates_exactly_one_task` failed after reporting `unexpected intake status: 500 Internal Server Error`; the step exited `101`. No rerun was requested or performed, so this hosted regression signal remains unresolved.

## Product-Tree Identity Versus Exact-Tip CI

The literal Git trees differ because `e161e77` adds `.omo` F3 evidence and knowledge. The product trees do not differ:

- `git diff --quiet 0fb4eb5..e161e77 -- . ':(exclude).omo'` passed.
- `git diff --name-status 0fb4eb5..e161e77` lists only `.omo/evidence/videnoa-controller/final/F3-manual-visual.md` and `.omo/knowledges/videnoa-controller-f3-manual-visual.md`.
- The worktree is also clean outside `.omo/`.
- The previously recorded Controller product identities remain unchanged: `controller-web` tree `1e21aa9ac5a1546ecafacf50473ef1c10afed070` and `crates/controller` tree `49f0a2f356e0ca9eb36c446a5a89679c9d242ebc`.

Therefore:

- Run `33933923401` is authoritative proof that the unchanged product revision passed all 14 jobs, including the packaging remediation.
- Run `33940048214` is authoritative proof that the literal current repository tip did not complete an all-green required matrix.
- Product-tree identity is relevant evidence against an intentional source regression, but it cannot erase a failed exact-tip required check or establish that the failing concurrency path is now reliable.

## Packaging and Release Acceptance

### Legacy Linux packaging remediation

The old `d8830fa` F4 report correctly rejected its then-current run because p7zip failed before archive verification. That state is superseded:

- `fd05465` introduced the resource-bounded helper path.
- The helper preserves the `videnoa/` root, `videnoa-linux64-<version>.7z*` names, 2000 MiB split-volume convention, `.7z.001` verification entry, and unsplit fallback.
- Historical real-bundle evidence records successful creation and verification of a roughly 5.33 GB bundle using `-mx=5 -md=16m -mmt=1`, with a 2,097,152,000-byte `.001` and a 151,790,351-byte `.002`. This audit inspected that existing evidence; it did not repeat the multi-gigabyte execution.
- Run `33933923401` passed legacy Linux archive creation and verification on `0fb4eb5`.
- Current-tip run `33940048214` again passed legacy Linux archive contracts, bundle construction, runtime compatibility, cache reclamation, split-archive creation, and layout/split-volume verification.

Fresh local command:

```bash
bash scripts/tests/package_dist_archive_test.sh
```

Result: PASS. Split, single, resource-safe command, missing verification output, missing creation output, insufficient-space, and fatal-create propagation contracts passed.

### Controller archives and image

Current-tip hosted jobs passed:

- `videnoa-controller-v0.1.2-linux-x86_64.tar.gz` build and verification;
- `videnoa-controller-v0.1.2-windows-x86_64.zip` native Windows build and verification;
- `Dockerfile.controller` image build plus complete image/content smoke.

The historical Task 22/23 and final-preflight evidence was inspected rather than re-executed locally. It records exact archive allowlists, deterministic Linux output, embedded SPA startup, numeric non-root `10001:10001`, persistent mounts, expected C runtime linkage, and rejection of legacy binaries, loose frontend assets, models, ORT, CUDA, cuDNN, TensorRT, NVIDIA runtime content, caches, keys, and certificates. Current hosted success is the native/current execution authority; this audit did not build a new local image, Linux production archive, or Windows archive.

### Release graph and existing-product preservation

Fresh local command:

```bash
node scripts/tests/validate_ci_release_workflows.test.mjs
```

Result: PASS. The complete positive matrix and all negative mutations passed, including existing-product break detection, Controller Dockerfile/Rust asset/version/archive/tag checks, forbidden GPU-content validation, and helper-bypass rejection in both Linux smoke and release jobs.

The validated release graph retains separate outputs:

- Controller images: `controlnet/videnoa-controller:0.1.2` and `controlnet/videnoa-controller:latest`.
- Controller archives: `videnoa-controller-v0.1.2-linux-x86_64.tar.gz` and `videnoa-controller-v0.1.2-windows-x86_64.zip`.
- Existing images: `controlnet/videnoa:0.1.2` and `controlnet/videnoa:latest`.
- Existing archives: `videnoa-linux64-0.1.2.7z*` and `videnoa-win64-0.1.2.7z*`, retaining the `videnoa/` layout.

The release version gate still compares the app, Controller, core, and desktop manifests. GitHub Release creation still depends on both legacy archives, both Controller archives, and both image publication paths. No new credential scheme is introduced.

## Regression, Migration, Backup, and Rollback Coverage

- Current-tip hosted Rust, Web, legacy package, Docker, Controller Rust/Web/archive/image, and crash/outage jobs passed.
- The current-tip fault/load job did not pass as a whole because of the duplicate-intake concurrency failure described above.
- Existing exact-product evidence records successful fresh migration, migration-5 upgrade with data preservation, and injected migration rollback without partial schema or false success recording.
- Existing documentation contracts cover full `data_root` plus WAL/SHM backup, matching worker `jobs.db` and workspaces, startup migration verification, health/readiness and retained-history checks, and rollback by restoring the complete pre-upgrade Controller/worker snapshots with the previous binary/image/configuration. No unsupported downgrade command is claimed.
- No migration, backup/restore drill, production publication, or release rollback was newly executed by this audit. These findings rely on inspected existing evidence plus unchanged product trees and the current hosted Controller Rust job.

## Scope and Freshness Limitations

- No GitHub Release was created, no release tag was created, and no Docker Hub image was published or pulled by this audit. Both inspected `dev` runs are smoke/quality workflows, not production releases.
- The Actions artifacts API reports `total_count: 0` for both runs. No retained downloadable package is claimed.
- The multi-gigabyte local legacy archive, local Controller image, local production Controller archives, native Windows execution, migration drill, and backup/rollback drill were not repeated. Their historical evidence is explicitly attributed above.
- The two required focused local contract suites were run fresh against the current worktree and passed.
- The current worktree contains uncommitted approval/evidence changes. F1 and F2 are approvals of `0fb4eb5`; F3 separately bridges unchanged product trees to `e161e77`. None of those dirty reports is represented as committed repository state.
- English and Chinese are the user-authorized initial language scope. This F4 audit makes no Korean-support claim.

## Remaining Landing Blockers

1. Obtain an all-green required hosted run for the literal commit that is to be landed. At present the exact current-tip run `33940048214` is red because the duplicate-intake concurrency test returned HTTP 500.
2. Commit and push the intended final F1/F2/F3/F4 reports, knowledge/notepad updates, and plan state as an intentional reviewable change, then verify `HEAD`, the actual remote tip, and the worktree are all clean and aligned. This audit was explicitly forbidden from performing that landing work.

VERDICT: REJECT
