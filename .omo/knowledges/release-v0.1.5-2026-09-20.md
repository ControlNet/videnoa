# v0.1.5 release preparation

## Candidate

- Release branch: `release/0.1.5`, based on dev `cf902e3`.
- The exact dev source passed all 14 jobs in
  https://github.com/ControlNet/videnoa/actions/runs/35458066258, including
  Linux/Windows Rust and archives, both frontends, Controller fault/load suites,
  and both Docker checks.
- All five workspace packages and their lockfile entries advance from 0.1.4 to
  0.1.5. No external dependency version changes are intended by release prep.
- Formal publication follows the existing master-triggered Release Workflow after
  candidate CI passes. The workflow creates the version tag; do not create it in
  advance.

## Release notes

- Rebuild the Worker Models, model detail, Performance, and Settings views with
  clearer model graphs and controls.
- Add configurable Controller DATA ROOT and CACHE ROOT paths, including staged
  restart-safe migration and durable root discovery.
- Add a locally persisted Controller task-table view and an optional priority
  column, while keeping task refreshes and the Settings commit bar stable.
- Automatically retry uncertain task submissions with the same idempotent
  identity, and harden recovery against concurrent Worker health updates.
- Preserve publication destinations correctly during verified cross-filesystem
  copies and clean up newly created empty outputs after failures.
- Align the Worker Docker GPU runtime with the release environment: CUDA 12.8,
  cuDNN 9.14, TensorRT 10.9, and ONNX Runtime 1.23.2. This fixes the RIFE v4.26
  TensorRT cold-cache compilation mismatch observed with the previous image.
- Reduce avoidable Docker payload by excluding the build context, removing the
  duplicate Worker WebUI payload, and retaining only required MKVToolNix tools.
- Move CI and action runtimes to Node.js 24 and repair workflow/archive contracts
  for the current Docker and Controller path configuration.

## Upgrade notes

- Back up Worker and Controller persistent data before upgrading. Update Worker
  and Controller together when they share operational workflows.
- Existing relative Controller roots still default to `data`. If custom roots are
  configured, keep the default data mount for its locator, persist both DATA ROOT
  and CACHE ROOT, pause scheduling, wait for terminal tasks, then restart after a
  path change.
- The updated TensorRT runtime may compile compatible engines on first use. Keep
  model and cache storage persistent and allow the initial cold run to finish.
- Docker runtime alignment intentionally retains the complete libraries required
  by the published inference backends; image size is not expected to match the
  earlier incomplete runtime image.

## Candidate verification

```bash
cargo metadata --locked --offline --format-version 1 --no-deps
cargo fmt --all -- --check
node scripts/tests/validate_ci_release_workflows.test.mjs
bash scripts/tests/controller_docs_test.sh
bash scripts/tests/controller_archive_root_files_test.sh
bash scripts/tests/package_controller_test.sh
bash scripts/tests/docker_slimming_contract_test.sh
git diff --check
```

Expected: five workspace packages resolve to 0.1.5 and all commands exit zero.
Release preparation changes only the workspace version, five workspace lockfile
versions, this release record, and the stale documentation contract discovered by
the candidate checks. Candidate CI and published artifact verification are
recorded separately after they actually complete.
