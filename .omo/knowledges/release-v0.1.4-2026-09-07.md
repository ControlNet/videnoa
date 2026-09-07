# v0.1.4 release preparation

## Candidate

- Release branch: `release/0.1.4`, based on dev `c9c4f2c`.
- The last code change, `ec203a6`, passed all 14 jobs in https://github.com/ControlNet/videnoa/actions/runs/34123916427, including Linux/Windows Rust and archives, both frontends, Controller fault/load suites, and Docker checks.
- All five workspace packages and their lockfile entries are advanced from 0.1.3 to 0.1.4. No external dependency version changes are intended.
- Formal publication follows the existing master-triggered Release Workflow after candidate CI passes. The workflow creates the version tag; do not create it in advance.

## Release notes

- Add password-authenticated iroh connections between Controller and Workers, persistent endpoint identities, and endpoint ID logging when Worker iroh starts.
- Correct iroh disable/reenable behavior, health authentication classification, and Windows dependency compatibility.
- Add Jellyfin-compatible version suffixes for batch output names.
- Improve live Worker/task updates, Worker-name filters, connection-type selection, and table/detail stability.
- Accept current input contents at upload time instead of rejecting changes since task creation. Persist current upload size for recovery and allow users to retry historical `input_changed` failures without editing the database.
- Bundle Manrope and Geist Mono fonts and licenses locally in both frontends, removing Google Fonts runtime requests.
- Improve MKV metadata-tool error diagnostics.

## Upgrade notes

- Update Controller and Workers together to use the new iroh connection mode. Existing HTTP connections remain supported.
- Retain existing data directories and database files. Persist Worker/Controller `iroh.key` identity files when using iroh; never publish them. First iroh activation requires a Worker password.
- Historical input-change failures require an explicit Retry action; they do not automatically restart.
- Local fonts remove the browser font-service dependency, but iroh still uses public N0 discovery/relay defaults. Earlier NAS relay Ping timeout observations are not proof of a release defect; real-network behavior remains environment-dependent.
- Use the established upgrade backup procedure before starting the new binaries against persistent state.

## Local verification

```bash
cargo metadata --locked --offline --format-version 1 --no-deps
cargo fmt --all -- --check
node scripts/tests/validate_ci_release_workflows.test.mjs
bash scripts/tests/controller_docs_test.sh
git diff --check
```

Expected: five workspace packages resolve to 0.1.4, all commands exit zero, and only the workspace version and five workspace lockfile versions change besides this release record. Release candidate CI and published artifact verification must be recorded separately after they actually complete.
