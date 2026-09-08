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

Expected: five workspace packages resolve to 0.1.4, all commands exit zero, and the version bump changes only the workspace version and five workspace lockfile versions. Review follow-up fixes are listed below. Release candidate CI and published artifact verification must be recorded separately after they actually complete.

## Candidate review follow-up

- PR: https://github.com/ControlNet/videnoa/pull/2.
- Public N0 Endpoint-ID-only transport test passed on the v0.1.4 candidate: `CARGO_TARGET_DIR=/tmp/videnoa-iroh-target cargo test --locked -p videnoa-transport n0_endpoint_id_only_connection -- --ignored`. This checks public discovery from the current host, not mainland/NAS-to-GPU or forced-relay throughput.
- Review identified a real Worker delta race: two queued microtasks derived new lists from stale effect closures, so the second update could undo the first. A synthetic two-Worker regression reproduced `[true, false]` instead of `[false, false]` before the fix.
- The follow-up uses a functional state update and checks the target Worker's version against the latest list. Unlike the initial version-only candidate, the final candidate includes this narrowly scoped frontend correction and its regression test.
- Verification: `npm --prefix controller-web test -- src/workers`, `npm --prefix controller-web run lint`, and `npm --prefix controller-web run build` must pass before pushing the corrected candidate.

## Publication concurrency correction

- Candidate PR CI run 34127608699 failed `duplicate_publication_finalizers_preserve_exactly_one_final_artifact`: zero finalizers completed. The parallel push run passed, exposing a timing-dependent race rather than a consistently broken packaging step.
- Two finalizers for one task could race the source rename and durable completion transition; the duplicate could mark the task failed while the first was still completing publication.
- A deterministic regression pauses the first publisher before destination staging and requires the duplicate to return `TransferError::Busy`. Before the fix the duplicate returned `Ok(Completed)`; after the fix all 18 filesystem tests passed, including both abort-and-recovery scenarios.
- The shared transfer coordinator now owns a per-task RAII publication permit. It covers snapshots, rename, completion, and cleanup, and releases on completion or cancellation. Independent tasks retain concurrency. This is an in-process guard under the existing single-Controller ownership model.
- Test artifacts are synthetic test-only bytes; production publication code is exercised.

Verification commands for the corrected candidate:

```bash
CARGO_TARGET_DIR=/tmp/videnoa-iroh-target cargo test --locked -p videnoa-controller --all-targets
CARGO_TARGET_DIR=/tmp/videnoa-iroh-target cargo clippy --locked -p videnoa-controller --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Expected: no test failures, no Clippy warnings, and clean formatting. Final cross-platform CI remains a prerequisite for merging PR #2.

- The local default-parallel full test run passed 32/33 task20 cases but timed out the three-Worker case in `Submitting` (before publication), with one remote run and zero polls. The same case passed in isolation in 15.62 seconds. Full validation is repeated with `-- --test-threads=4` to limit contention between fault scenarios, preserving each scenario’s internal concurrency; the unchanged CI command remains a separate gate.
