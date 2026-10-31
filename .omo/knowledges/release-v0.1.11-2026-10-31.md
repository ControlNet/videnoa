# v0.1.11 release preparation

## Candidate

- Release branch: `release/0.1.11`, based on dev `b6ed5b0` (87 non-merge
  commits since `v0.1.10`: the post-v0.1.10 review fixes, npm advisory
  fixes, and every finding of the 2026-10-30 design review, see
  `design-review-2026-10-30.md`).
- The maintainer chose 0.1.11 rather than 0.2.0 despite protocol 2 and the
  config/DB version bumps.
- Version bump as for v0.1.10: `[workspace.package] version` in `Cargo.toml`,
  then `cargo update -w --offline` (six workspace entries in `Cargo.lock`
  now, with `videnoa-protocol`).
- Publication follows the master-triggered Release Workflow after candidate CI
  passes; the workflow creates the `v0.1.11` tag. Replace the auto-generated
  body with `release-v0.1.11-body.md` afterwards.
- The desktop data directory is unchanged: the app-data move from a7ecbf7 was
  reverted (af0ae22) before the release.

## Candidate verification

The commands from `release-v0.1.9-2026-10-03.md`, plus
`cargo deny check advisories` (advisories ok on 2026-10-31).

- `scripts/tests/controller_docs_test.sh` (not run by CI) failed: the
  protocol 2 docs rewrite (7df0e28) had shortened the task action routes to
  "`cancel`, `retry`, `abandon` and `discard`", so the check could not find
  `/api/tasks/{id}/retry`. Fixed on the release branch by writing each full
  route; the check now also requires the abandon and discard routes.
