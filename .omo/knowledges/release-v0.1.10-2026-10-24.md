# v0.1.10 release preparation

## Candidate

- Release branch: `release/0.1.10`, based on dev `23f650b` (43 non-merge
  commits since `v0.1.9`: queued retries and retry on another Worker, the
  mkvmerge MKV mux, both Codex worker review rounds, the Codex controller
  review fixes and two RUSTSEC dependency bumps), plus the RUSTSEC-2026-0386
  tokio bump made on the release branch after the first candidate run.
- Version bump as for v0.1.9: `[workspace.package] version` in `Cargo.toml`,
  then `cargo update -w --offline` (five workspace entries in `Cargo.lock`).
  No README or docs version-specific notes needed changing.
- Publication follows the master-triggered Release Workflow after candidate CI
  passes; the workflow creates the `v0.1.10` tag. Replace the auto-generated
  body with `release-v0.1.10-body.md` afterwards.
- No open issues are addressed by this release.

## Compatibility checked for the upgrade notes

- Controller migration 0014 (`tasks.requested_worker_id`). The sqlx
  `Migrator` rejects a database that has applied a migration it does not
  know, so a v0.1.9 Controller cannot open a migrated database; downgrading
  needs the pre-upgrade backup.
- MKV outputs fail before any frame when mkvmerge is missing
  (`video_output.rs` `for_config_with`). The Linux bundle, the Worker image
  and the Windows bundle ship it.
- `PUT /api/files` still answers `{path,size}`; the staged upload only changes
  what the target holds while a body is in flight. The Codex controller
  review checked that the Controller's upload resume copes with that, and with
  outputs that are published only on success
  (`codex-controller-review-2026-10-23.md`).
- Retry on another Worker uses the workflow validation from v0.1.9; Workers
  older than v0.1.9 answer 404 and stay eligible.
- `RetryTaskResponse.attempt_id` is `null` for a queued retry.

## Candidate verification

The same commands as for v0.1.9 (`release-v0.1.9-2026-10-03.md`), run on the
release branch before pushing it.
