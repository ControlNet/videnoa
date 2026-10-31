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

## Compatibility checked for the upgrade notes

Drafted by a subagent against the code on `dev` and the v0.1.10 tag; the
curated body is `release-v0.1.11-body.md`.

- **Upgrade the Controller first.** A v0.1.10 Controller treats any 403 from
  a Worker as a wrong password and stops health checks until the
  registration is edited (v0.1.10 `workers/health/probe.rs`,
  `workers/health.rs`), so a passwordless v0.1.11 Worker reached by a DNS
  name it does not allow can get stuck offline behind an old Controller.
- v0.1.11 Controller with v0.1.9/v0.1.10 Workers: protocol-1 tests in
  `crates/controller/tests/task20/worker_protocol.rs` (mock Worker).
- v0.1.11 Workers need Controller v0.1.9 or later: v0.1.8's remote DTOs use
  `deny_unknown_fields`.
- Downgrading a Worker to v0.1.10: it opens the new `jobs.db` (it never reads
  `user_version`) and ignores `schema_version` in saved workflows. But once
  v0.1.11 has saved settings, `config.toml` says `workflows_dir =
  "workflows"`, which v0.1.10 resolves against the working directory; restore
  the backed-up `config.toml`. The reverse trap also exists: v0.1.10 saving
  the config drops `version`, so v0.1.11 then reads it as version 1 and pins
  `workflows` to `<cwd>/workflows`.
- Controller migrations 0015/0016: sqlx `Migrator` refuses a database with
  migrations it does not know, so a v0.1.10 Controller cannot open it.
- No Compose example in the repo relies on a passwordless Worker reached by
  service name, so the Host check note in Upgrade is enough.
