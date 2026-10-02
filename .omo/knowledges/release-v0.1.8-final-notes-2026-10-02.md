# v0.1.8 final release notes

## Published result

- Release: https://github.com/ControlNet/videnoa/releases/tag/v0.1.8
  (published 2026-10-02T06:46:31Z). The auto-generated body was replaced with
  the curated notes in `release-v0.1.8-body.md` (same layout as v0.1.7).
- Candidate run https://github.com/ControlNet/videnoa/actions/runs/36966315931
  passed all 15 jobs for `release/0.1.8` at `9a92a44`.
- Release Workflow run
  https://github.com/ControlNet/videnoa/actions/runs/36968054611 succeeded on
  the first attempt, including Verify Release Outcome.
- Tag `v0.1.8` (annotated, object `db41481`) and `origin/master` both resolve
  to `6cd271a34ae69a996170aba704d7590805059fd4`.

## Published assets

- `videnoa-linux64-0.1.8.7z.001` — 2,097,152,000 bytes
- `videnoa-linux64-0.1.8.7z.002` — 280,635,125 bytes
- `videnoa-win64-0.1.8.7z.001` — 1,727,863,875 bytes
- `videnoa-controller-v0.1.8-linux-x86_64.tar.gz` — 14,594,596 bytes
- `videnoa-controller-v0.1.8-windows-x86_64.zip` — 11,722,507 bytes

`docker buildx imagetools inspect` shows the same digest for the version tag
and `latest`:

- `controlnet/videnoa:0.1.8` and `:latest`: `sha256:dc4ea160f2f6565c79049714a8ebdb021665f651bf8292656c2194bc0ef3fe49`
- `controlnet/videnoa-controller:0.1.8` and `:latest`: `sha256:6fd0a942a650c9f28da4412ad900700414ec2d7e887c2f1d5c8a25b91ce4f1cf`

## GitFlow completion

- `release/0.1.8` was merged into `master` (`--no-ff`) only after candidate CI
  passed; published `master` was merged back into `dev`; the release branch
  was deleted locally and on origin.
- No issues were tied to this release. The v0.1.7 known issue (NVENC 64x64
  probe) is fixed.
- Housekeeping before the release: merged `origin/feat/optional-password`
  deleted; local tag `v0.1.2` differs from origin's (left untouched), so use
  `git fetch origin` without `--tags`.
