# v0.1.9 release preparation

## Candidate

- Release branch: `release/0.1.9`, based on dev `179ead0` (18 non-merge
  commits since `v0.1.8`: self-hosted iroh relays with Settings UI and the
  optional public relays, workflow validation at task creation, manual retry
  of rejected submissions, seek-based previews, per-job download isolation,
  lenient Worker response DTOs, Worker web CI).
- Version bump as for v0.1.8: `[workspace.package] version` in `Cargo.toml`,
  then `cargo update -w --offline` (five workspace entries in `Cargo.lock`).
  No README version-specific notes needed changing.
- Publication follows the master-triggered Release Workflow after candidate CI
  passes; the workflow creates the `v0.1.9` tag. Replace the auto-generated
  body with `release-v0.1.9-body.md` afterwards.
- No open issues are addressed by this release.

## Compatibility checked for the upgrade notes

- v0.1.9 Controller + v0.1.8 Worker: unknown response fields are ignored
  (`02742ce`); `POST /api/run/validate` answers 404 on old Workers, which
  keeps their workflows eligible (`91d97cb`).
- v0.1.9 Worker + older Controller: a Worker unit test pins the response
  field sets v0.1.8 and older Controllers require.
- iroh: empty `relay_urls` keeps the N0 preset, so existing deployments are
  unchanged.

## Candidate verification

```bash
cargo metadata --locked --offline --format-version 1 --no-deps
cargo fmt --all -- --check
git ls-files -z '*.rs' | xargs -0 -n1 rustfmt --edition 2021 --check
node scripts/tests/validate_ci_release_workflows.test.mjs
node scripts/validate_ci_release_workflows.mjs
bash scripts/tests/controller_docs_test.sh
bash scripts/tests/controller_archive_root_files_test.sh
bash scripts/tests/package_controller_test.sh
bash scripts/tests/docker_slimming_contract_test.sh
git diff --check
```

`scripts/tests/controller_docs_test.sh` (not run by CI) resolves links in
the Controller docs relative to the repository root, so a same-directory
link such as `iroh.md#...` in `docs/controller.md` fails; write it as
`../docs/iroh.md#...`, which is also valid relative to `docs/`.
