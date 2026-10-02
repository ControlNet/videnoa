# v0.1.7 final release notes

## Published result

- Release: https://github.com/ControlNet/videnoa/releases/tag/v0.1.7
  (published 2026-10-01T06:27:32Z). The auto-generated body was replaced with
  the curated notes in `release-v0.1.7-body.md` (same layout as v0.1.6).
- Candidate run https://github.com/ControlNet/videnoa/actions/runs/36816880700
  passed all 14 jobs for `release/0.1.7` at `0b8e30d`.
- Release Workflow run
  https://github.com/ControlNet/videnoa/actions/runs/36818521271 succeeded on
  the first attempt, including Verify Release Outcome.
- Tag `v0.1.7` and `origin/master` both resolve to
  `5cc7d56ff651324e3927485846bda7960cf42256`.

## Published assets

- `videnoa-linux64-0.1.7.7z.001` — 2,097,152,000 bytes
- `videnoa-linux64-0.1.7.7z.002` — 280,588,857 bytes
- `videnoa-win64-0.1.7.7z.001` — 1,727,740,535 bytes
- `videnoa-controller-v0.1.7-linux-x86_64.tar.gz` — 14,690,012 bytes
- `videnoa-controller-v0.1.7-windows-x86_64.zip` — 11,877,804 bytes

`docker buildx imagetools inspect` shows the same digest for the version tag
and `latest`:

- `controlnet/videnoa:0.1.7` and `:latest`: `sha256:a01a93659141fb73bee0d9728b54d34fead990a824c9f1d96e3dae6f73651f23`
- `controlnet/videnoa-controller:0.1.7` and `:latest`: `sha256:6dd5990a52581660cdd5d216cc63b1f1bf82bb8e0642301a90501e4edbff1dcc`

## GitFlow completion

- `release/0.1.7` was merged into `master` only after candidate CI passed.
- Published `master` was merged back into `dev` after release verification.
- No issues were tied to this release.
- Known issue carried forward: the NVENC 64x64 availability probe fails on
  GPUs such as the A40 (parked).
