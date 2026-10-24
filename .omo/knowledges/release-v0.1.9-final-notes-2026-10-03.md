# v0.1.9 final release notes

## Published result

- Release: https://github.com/ControlNet/videnoa/releases/tag/v0.1.9
  (published 2026-10-03T03:55:08Z). The auto-generated body was replaced with
  the curated notes in `release-v0.1.9-body.md` (same layout as v0.1.8).
- Candidate run https://github.com/ControlNet/videnoa/actions/runs/37088489256
  passed all 15 jobs for `release/0.1.9` at `5508af4`.
- Release Workflow run
  https://github.com/ControlNet/videnoa/actions/runs/37090084454 succeeded on
  the first attempt (25 jobs, including Verify Release Outcome).
- Tag `v0.1.9` (annotated, object `33685bd`) and `origin/master` both resolve
  to `8c90cbb4505991006daf3503a61fd456a95dcb21`.

## Published assets

- `videnoa-linux64-0.1.9.7z.001` — 2,097,152,000 bytes
- `videnoa-linux64-0.1.9.7z.002` — 280,709,308 bytes
- `videnoa-win64-0.1.9.7z.001` — 1,727,952,589 bytes
- `videnoa-controller-v0.1.9-linux-x86_64.tar.gz` — 14,617,141 bytes
- `videnoa-controller-v0.1.9-windows-x86_64.zip` — 11,825,538 bytes

`docker buildx imagetools inspect` shows the same digest for the version tag
and `latest`:

- `controlnet/videnoa:0.1.9` and `:latest`: `sha256:5b9705f24c8ef7a80f6f382dac0cddec0a0de90ffb369cb5e7efba11199b1f42`
- `controlnet/videnoa-controller:0.1.9` and `:latest`: `sha256:ea3ef81108b0ee484c01ca2e1bf15f545b7864b3ab3e76e0902e0c18d0eb9181`

## GitFlow completion

- `release/0.1.9` was merged into `master` (`--no-ff`) only after candidate CI
  passed; published `master` was merged back into `dev`; the release branch
  was deleted locally and on origin.
- No issues were tied to this release.

## Open follow-up (not verified)

- Preview sessions cap at 256 MiB including the full-resolution extracted
  PNGs; ten 4K frames plus one 2x super-resolved result may exceed it.
  Check with a real 4K source before changing the limit.
