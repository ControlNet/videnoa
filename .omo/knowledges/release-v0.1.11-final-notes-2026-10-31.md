# v0.1.11 final release notes

## Published result

- Release: https://github.com/ControlNet/videnoa/releases/tag/v0.1.11
  (published 2026-10-31T10:18:15Z). The auto-generated body was replaced with
  `release-v0.1.11-body.md`.
- Candidate runs on `release/0.1.11`:
  - 41182303452 at `e8aa79b`: cancelled by the next push (release notes).
  - 41182922576 at `280cd66`: failed only "Controller web quality and E2E".
    `AuthApp.test.tsx` "authenticates setup without browser-stored secrets"
    checked localStorage right after the Tasks heading appeared, but the task
    view is stored by a passive effect that can run later under load. Not
    reproducible locally (5 single runs, 6 parallel full suites). Fixed in
    `dbb4bd6` with `waitFor`; test-only.
  - 41184903390 at `dbb4bd6`: all jobs passed.
- Release Workflow run 41186371943 succeeded on the first attempt (25 jobs;
  "Release Skipped Summary" is skipped by design when a release is made).
- Tag `v0.1.11` and `origin/master` both resolve to
  `3a5e8da6df0620c304af5bb5da9e4487d416ced6`.

## Published assets

- `videnoa-linux64-0.1.11.7z.001` — 2,097,152,000 bytes
- `videnoa-linux64-0.1.11.7z.002` — 280,993,506 bytes
- `videnoa-win64-0.1.11.7z.001` — 1,733,979,258 bytes
- `videnoa-controller-v0.1.11-linux-x86_64.tar.gz` — 14,812,680 bytes
- `videnoa-controller-v0.1.11-windows-x86_64.zip` — 11,986,842 bytes

`docker buildx imagetools inspect` shows the same digest for the version tag
and `latest`:

- `controlnet/videnoa:0.1.11` and `:latest`: `sha256:1a064d62a9f717140c67510ba41ba0e5c7e6b58b80f9b207aa965d0718d2fa18`
- `controlnet/videnoa-controller:0.1.11` and `:latest`: `sha256:e242ea60b9136694e22afa3065dc695d005aa88229d52bbbdf9c1dd231cbb202`

## GitFlow completion

- `release/0.1.11` was merged into `master` (`--no-ff`) only after candidate
  CI passed; published `master` was merged back into `dev`; the release branch
  was deleted locally and on origin.
- No issues were tied to this release.

## Not fixed in this release

- Open items of the design review (`design-review-2026-10-30.md`): dynamic-shape
  TensorRT engines built at first `Run()` are outside the lock and not
  recovered when truncated; on Unix FFmpeg can outlive the Worker; the
  cancel-lookup race with a crashed Controller's in-flight run; retention can
  purge a discarded task whose Worker folder was kept.
- Post-v0.1.10 review items 3, 4 and 5 stay unfixed by the maintainer's choice.
