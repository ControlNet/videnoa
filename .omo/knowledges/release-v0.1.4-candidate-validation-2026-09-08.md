# v0.1.4 candidate validation

- PR: https://github.com/ControlNet/videnoa/pull/2, `release/0.1.4` into `master`.
- Final candidate: `8648494dc68b56c488fa56d2784e10a181253e01`.
- All 14 jobs passed in both candidate runs:
  - PR: https://github.com/ControlNet/videnoa/actions/runs/34130781612
  - Push: https://github.com/ControlNet/videnoa/actions/runs/34130777716
- Coverage includes both frontends, Linux/Windows Rust, Controller full tests, fault/load/security suites, both platform archives, and Worker/Controller Docker checks.
- All five workspace versions are 0.1.4 with no external dependency version changes.
- Candidate review fixed two reproduced races: queued Worker deltas overwriting each other, and duplicate publication finalizers interfering with the source rename and completion transition. The latter uses a per-task RAII permit in the shared transfer coordinator; cancellation recovery tests pass.
- Local Controller validation: 574 tests passed with four test threads; the separately enabled production Argon2 contention test passed. All-target/all-feature Clippy, formatting, workflow contracts, documentation checks, and staged secret scan passed.
- The first local default-parallel run timed out one three-Worker task20 case while still Submitting, before publication. Its isolated run and bounded-parallel full run passed; both unchanged CI fault/load jobs also passed. No test timeout or assertion was relaxed.
- Public N0 Endpoint-ID-only connection test passed locally on the v0.1.4 candidate before the independent UI/publication corrections. This does not verify mainland/NAS-to-GPU connectivity or forced-relay throughput.

## Reproduce local checks

Run in `/tmp/videnoa-release-0.1.4`:

```bash
CARGO_TARGET_DIR=/tmp/videnoa-iroh-target cargo test --locked -p videnoa-controller --all-targets -- --test-threads=4
CARGO_TARGET_DIR=/tmp/videnoa-iroh-target cargo test --locked -p videnoa-controller --test auth_contention -- --ignored
CARGO_TARGET_DIR=/tmp/videnoa-iroh-target cargo clippy --locked -p videnoa-controller --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
node scripts/tests/validate_ci_release_workflows.test.mjs
bash scripts/tests/controller_docs_test.sh
```

Expected: all commands exit zero; 574 default tests and one explicitly enabled contention test pass.

## Handoff

Release preparation is complete; the release PR remains open. No tag or published release was created. Merging into master triggers the existing Release Workflow, which creates the version tag and publishes artifacts. Do not pre-create the tag. Preserve data directories and iroh identity files during upgrades; update Controller and Workers together for iroh use. Full release notes are in the PR and the release branch knowledge record.
