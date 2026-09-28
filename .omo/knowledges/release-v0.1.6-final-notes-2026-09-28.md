# v0.1.6 final release notes

## Published result

- Release: https://github.com/ControlNet/videnoa/releases/tag/v0.1.6
  (published 2026-09-28T12:26:05Z; the body is `generate_release_notes`, the
  curated notes are in `release-v0.1.6-2026-09-28.md`).
- Successful Release Workflow:
  https://github.com/ControlNet/videnoa/actions/runs/36412872782 (all jobs
  including Verify Release Outcome).
- Tag `v0.1.6` and `origin/master` both resolve to
  `a417de77732ebd2e0edb0d95c6ed8dc8081080bb`.

## Published assets

- `videnoa-linux64-0.1.6.7z.001` — 2,097,152,000 bytes
- `videnoa-linux64-0.1.6.7z.002` — 280,554,780 bytes
- `videnoa-win64-0.1.6.7z.001` — 1,727,653,998 bytes
- `videnoa-controller-v0.1.6-linux-x86_64.tar.gz` — 14,691,601 bytes
- `videnoa-controller-v0.1.6-windows-x86_64.zip` — 11,877,972 bytes

`docker manifest inspect` shows the same digest for the version tag and
`latest`:

- `controlnet/videnoa:0.1.6` and `:latest`: `sha256:9ccb4761b795878d98df6bce7bc6da3b20aef049dca4f01229d1258a424c455c`
- `controlnet/videnoa-controller:0.1.6` and `:latest`: `sha256:68fb2b0baafbe5b58d8c34cb146343f63053d21f9e46daa42f5c358d4c26f709`

## Windows media tools in the published archive

Downloaded `videnoa-win64-0.1.6.7z.001` and extracted `videnoa/bin/*` with
`7z`. All entries in `SHA256SUMS` passed. The manifest has CRLF line endings,
so check it with `tr -d '\r' < SHA256SUMS | sha256sum -c -`. `ffmpeg.exe` is the
BtbN n8.1.3 GPL build, SHA-256
`a35880a72ca511bbf595a28c6480c85218cdf1d6c1008511a848fc71dbad5527`, the same
binary that was verified in the candidate bundle (#6). The release Windows
packaging job's `media_tools.ps1 -Verify` step also passed.

## Release recovery

- Candidate run https://github.com/ControlNet/videnoa/actions/runs/36404437957
  passed all 14 jobs for `release/0.1.6` at `dc6a5fd`.
- The first publication run
  https://github.com/ControlNet/videnoa/actions/runs/36407378258 at `bc20ade`
  failed in the quality gate. On Windows,
  `config::tests::data_dir_defaults_to_data_dir` read `/env/path`: it raced
  `data_dir_uses_env_var_when_no_cli` on `VIDENOA_DATA_DIR`, a pre-existing
  flaky test. Every packaging and publishing job was skipped, and no tag was
  created.
- Fix `bfaf810` on `fix/config-env-test-race`, branched from master:
  - a shared `DATA_DIR_ENV_LOCK` serializes the two tests;
  - the test no longer changes HOME and XDG_CONFIG_HOME, which `data_dir`
    stopped reading.

  Locally it passed 200 runs at 16 threads. Fix-branch CI run 36409937791
  passed all 14 jobs. It was merged into master as `a417de7`, which
  republished.

## GitFlow completion

- `release/0.1.6` was merged into `master` only after candidate CI passed.
- The test fix was validated on its own branch before being merged into
  `master`.
- Published `master` was merged back into `dev` after release verification.
- Issues #4, #5 and #6 had status comments before the release. Closing them
  and replacing the auto-generated release body wait for the maintainer.
