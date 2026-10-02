# v0.1.8 release preparation

## Candidate

- Release branch: `release/0.1.8`, based on dev `9f3f027` (57 commits since
  `v0.1.7`: the BT.709 default, the 2026-10-01 project audit fixes and the
  Controller-Worker interop review).
- Version bump as for v0.1.7: `[workspace.package] version` in `Cargo.toml`,
  then `cargo update -w --offline` (five workspace entries in `Cargo.lock`).
- README colour notes corrected: the all-sizes BT.709 default for untagged
  sources (`a4ad098`) landed after the v0.1.7 tag, so it ships in v0.1.8.
- Publication follows the master-triggered Release Workflow after candidate CI
  passes; the workflow creates the `v0.1.8` tag. Replace the auto-generated
  body with the curated notes afterwards.
- No open issues are addressed by this release. The v0.1.7 known issue (NVENC
  probe at 64x64 rejected on the A40) is fixed (`bf6a337`).

## Release notes

Worker video pipeline

- Untagged sources decode as BT.709 at every size (v0.1.7: BT.601 for frames
  narrower than 1280 and at most 576 high).
- YUV to RGB decoding and previews use `accurate_rnd+full_chroma_int`
  (round-trip error about +-2 instead of +-3).
- NVENC availability probe encodes 256x256, is killed after 30 s and memoises
  successes per (codec, pix_fmt, preset, profile): NVENC works on the A40.
- Encoder failures quote the tail of FFmpeg's stderr; sink `execute()` errors
  are reported instead of a later, vaguer error.
- StreamOutput encodes 8-bit 4:2:0 H.264, which RTMP ingest accepts.
- RIFE falls back to pageable memory when pinned allocation fails.
- Wide-gamut primaries (BT.2020, DCI-P3, XYZ) are still not converted, but
  the Worker now logs a warning.
- Previews: the frame count comes from container metadata (no full decode)
  with its own 20 s budget, subprocesses are bounded, and the sampled stream is
  the one jobs decode.

Worker API and server

- Invalid frame chains (missing required inputs, wrong topology, unsupported
  nodes) are rejected with 400 when a job is submitted, for `POST /api/jobs`,
  `POST /api/run` and `videnoa run`. `POST /api/batch` is all-or-nothing.
- Request heads must arrive within 30 s (slow-loris); HTTP/1.1 only.
- API-created presets are capped at 256 (409 `preset_limit_reached`).
- `videnoa run` honours `paths.trt_cache_dir`.
- The editor palette no longer offers `ColorSpace` and `SceneDetect`, which
  could not run.
- The Worker Docker image ships the release FFmpeg 8.1 bundle instead of the
  distribution's FFmpeg 4.4.

Controller

- A rejected submission's task failure message includes the Worker's reason.
- Idle Worker connections are dropped after 20 s, before the Worker's 30 s
  keep-alive close, which could make a health probe fail.
- Login and Bearer limiting is checked before Argon2: after five failures in
  five minutes the peer gets 429, even with the correct password, until the
  window passes.
- Expired sessions are purged periodically.
- The SPA recovers when another tab rotates the CSRF proof, and checks a
  stream that still reports open after sleep or resume.
- Fewer database reads: one grouped query for the worker list, one read per
  change for all SSE subscribers; pool acquire timeout separated from the
  SQLite busy timeout.
- Same-port listener host changes return a clear 400.

Dependencies and CI

- `rustls` 0.23.45 (RUSTSEC-2026-0285); npm advisories cleared in `web/`
  and `controller-web/`.
- CI: SHA-pinned actions, `--locked`, workspace fmt/clippy/cargo-deny gate,
  job timeouts.

## Upgrade notes

- Untagged SD sources decode with BT.709 instead of BT.601.
- Workflows with invalid frame chains fail at submission instead of during the
  job. Controller tasks fail as `remote_submission_failed`, which cannot be
  retried: fix the workflow and create a new task.
- Upgrade the Controller together with Workers. A v0.1.7 Controller with a
  v0.1.8 Worker can occasionally mark the Worker offline for a second because
  of the keep-alive close.
- Cleartext HTTP/2 (h2c) clients must use HTTP/1.1; HTTPS reverse proxies are
  unaffected.

## Candidate verification

```bash
cargo metadata --locked --offline --format-version 1 --no-deps
cargo fmt --all -- --check
git ls-files -z '*.rs' | xargs -0 -n1 rustfmt --edition 2021 --check
node scripts/tests/validate_ci_release_workflows.test.mjs
node scripts/validate_ci_release_workflows.mjs
bash scripts/tests/controller_docs_test.sh
bash scripts/tests/controller_archive_root_files_test.sh
bash scripts/tests/package_controller_test.sh
bash scripts/tests/docker_slimming_contract_test.sh
git diff --check
```
