# v0.1.10 final release notes

## Published result

- Release: https://github.com/ControlNet/videnoa/releases/tag/v0.1.10
  (published 2026-10-24T02:48:43Z). The auto-generated body was replaced with
  the curated notes in `release-v0.1.10-body.md` (same layout as v0.1.9).
- First candidate run https://github.com/ControlNet/videnoa/actions/runs/40044724594
  failed only the Workspace quality gate: cargo-deny reported RUSTSEC-2026-0386
  (tokio 1.49 `AsyncFd` I/O safety), published after the last dev CI run.
  - `cargo update -p tokio` stopped at 1.50.0, which is still affected.
    `cargo update -p tokio --precise 1.54.1` also moved mio, socket2, libc and
    tokio-macros (`753706e`, on the release branch).
  - The workspace does not use `AsyncFd`, so the deprecation of its safe
    constructors does not trip `-D warnings`.
- Second candidate run https://github.com/ControlNet/videnoa/actions/runs/40047441835
  passed all 15 jobs for `release/0.1.10` at `401ef53`.
- Release Workflow run
  https://github.com/ControlNet/videnoa/actions/runs/40049339005 succeeded on
  the first attempt (25 jobs, including Verify Release Outcome).
  - `gh run watch` exited early with status 1 while Package smoke (Windows) was
    still running. Check `gh run view --json status` before treating that as a
    failure.
- Tag `v0.1.10` (annotated, object `c6c98ae`) and `origin/master` both resolve
  to `7f209dd50f7a3c5716760274170d53d9844110a6`.

## Published assets

- `videnoa-linux64-0.1.10.7z.001` — 2,097,152,000 bytes
- `videnoa-linux64-0.1.10.7z.002` — 280,764,117 bytes
- `videnoa-win64-0.1.10.7z.001` — 1,733,841,674 bytes
- `videnoa-controller-v0.1.10-linux-x86_64.tar.gz` — 14,693,388 bytes
- `videnoa-controller-v0.1.10-windows-x86_64.zip` — 11,852,051 bytes

`docker buildx imagetools inspect` shows the same digest for the version tag
and `latest`:

- `controlnet/videnoa:0.1.10` and `:latest`: `sha256:db85c61aa93aed186fe7fde6c39d7ee6e0640a4014af4455c759fc521eeabfbc`
- `controlnet/videnoa-controller:0.1.10` and `:latest`: `sha256:4fab78b935647596e1068bc57b1dcc4cafc5f8d88767aa04e784deacaf064e34`

## GitFlow completion

- `release/0.1.10` was merged into `master` (`--no-ff`) only after candidate
  CI passed; published `master` was merged back into `dev`; the release branch
  was deleted locally and on origin.
- No issues were tied to this release.

## Not fixed in this release

- Worker review findings 4, 5, 6, 8, 9 and N5 were deferred by the user;
  finding 7 is deliberately unchanged (`codex-project-review-2026-10-23.md`).
- Controller review findings 3 (editing a Worker endpoint redirects existing
  attempts) and 4 (the login limiter does not count concurrent attempts) were
  left unfixed at the user's request (`codex-controller-review-2026-10-23.md`).
