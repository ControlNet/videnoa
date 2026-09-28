# v0.1.5 final release notes

## Published result

- Release: https://github.com/ControlNet/videnoa/releases/tag/v0.1.5
- Successful Release Workflow:
  https://github.com/ControlNet/videnoa/actions/runs/35488844023
- Tag `v0.1.5` and `origin/master` both resolve to
  `2fb0813e1b34d0ebe077ac8277dc2417a88542b7`.
- The workflow's release verification passed after checking GitHub assets and
  pulling both products' version and `latest` Docker tags.

## Published assets

- `videnoa-linux64-0.1.5.7z.001` — 2,097,152,000 bytes
- `videnoa-linux64-0.1.5.7z.002` — 280,421,628 bytes
- `videnoa-win64-0.1.5.7z.001` — 1,721,796,830 bytes
- `videnoa-controller-v0.1.5-linux-x86_64.tar.gz` — 14,651,795 bytes
- `videnoa-controller-v0.1.5-windows-x86_64.zip` — 11,877,984 bytes

Independent `docker manifest inspect` checks passed for:

- `controlnet/videnoa:0.1.5`
- `controlnet/videnoa:latest`
- `controlnet/videnoa-controller:0.1.5`
- `controlnet/videnoa-controller:latest`

## Release recovery

- Candidate run https://github.com/ControlNet/videnoa/actions/runs/35484980882
  passed all 14 jobs.
- The first publication run
  https://github.com/ControlNet/videnoa/actions/runs/35486240752 failed because
  the Linux packaging script made only one attempt to download the 2 GB
  `lib_linux64.zip.001` prerequisite.
- Fix run https://github.com/ControlNet/videnoa/actions/runs/35487940226 passed
  all 14 jobs after adding bounded retry, retryable HTTP status handling,
  timeouts, and partial-file continuation.
- The successful publication reran the full quality gate and all publishing
  jobs. Linux packaging passed the previously failing download and completed the
  split release archive.

## GitFlow completion

- `release/0.1.5` was merged into `master` only after candidate CI passed.
- The release download retry was validated on its fix branch before being merged
  into `master` and republished.
- Published `master` was merged back into `dev` after release verification.
