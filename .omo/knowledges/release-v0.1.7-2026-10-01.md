# v0.1.7 release preparation

## Candidate

- Release branch: `release/0.1.7`, based on dev `952f9b4`.
- The version lives only in `[workspace.package] version` (`Cargo.toml:6`);
  every crate inherits it with `version.workspace = true`. Bump that line,
  then run `cargo update -w --offline` to refresh the five workspace entries in
  `Cargo.lock` without touching external dependencies.
- Publication follows the master-triggered Release Workflow after candidate CI
  passes. The workflow creates the `v0.1.7` tag; do not create it in advance.
  After publishing, replace the auto-generated body with the curated notes.
- No open issues are addressed by this release.

## Release notes

Worker video pipeline

- Fix output colours. VideoOutput converted RGB to YUV with swscale's default
  BT.601 matrix but tagged the stream BT.709, so players showed shifted
  colours: (0,200,0) came back as (0,167,0). VideoOutput, StreamOutput and
  previews now convert with BT.709 limited range and tag the range.
- Untagged sources are decoded with the player convention: BT.709 when the
  frame is at least 1280 wide or more than 576 high, otherwise BT.601.
  Previously every untagged source used BT.601. Tagged sources use their tag.
- Per-frame buffers are reused through a per-job frame pool instead of being
  allocated and freed for every frame. RIFE frame interpolation feeds the GPU
  from a reusable CUDA-pinned input buffer.
- The decoder's `avg_decode_ms` log now reports the real decode time instead of
  0.0.

Measured on an A40-24Q vGPU, 1080p 120-frame source, 4K output, warmed
TensorRT cache, base `8c2a60e` vs `952f9b4`, 3 runs each:

| Pipeline | v0.1.6 | v0.1.7 | Change |
|---|---:|---:|---:|
| SR -> FI, NVENC | 23.6 s | 14.5 s | -38% |
| SR -> FI, libx265 | 25.4 s | 20.1 s | -21% |
| SR only | 10.5 s | 9.4 s | -10% |

System time dropped from 47.7 s to 11.1 s (NVENC) and from 52.5 s to 13.7 s
(x265). Minor page faults dropped from about 20M to about 3M. Peak RSS of FI
jobs is 0.3-0.5 GB higher because the pool keeps buffers.

Development

- `scripts/setup_dev_media_tools.sh` installs the release FFmpeg bundle into
  `<repo>/bin`, so development runs use the same FFmpeg 8.1 as the packages.

## Upgrade notes

- Output colours change, because they are now correct. Outputs from earlier
  versions were colour-shifted, most visibly in saturated greens and reds.
- Untagged HD sources decode with BT.709 instead of BT.601.
- Frame interpolation jobs use 0.3-0.5 GB more host memory.

## Known issue (not addressed)

- The NVENC availability probe encodes a 64x64 frame, which some GPUs (for
  example the A40) reject, so NVENC is reported unavailable there. Parked by
  request.

## Candidate verification

```bash
cargo metadata --locked --offline --format-version 1 --no-deps
cargo fmt --all -- --check
node scripts/tests/validate_ci_release_workflows.test.mjs
node scripts/validate_ci_release_workflows.mjs
bash scripts/tests/controller_docs_test.sh
bash scripts/tests/controller_archive_root_files_test.sh
bash scripts/tests/package_controller_test.sh
bash scripts/tests/docker_slimming_contract_test.sh
git diff --check
```

Result: every command passed locally on 2026-10-01, and the five workspace
packages resolve to 0.1.7. The candidate CI result and the published
artifacts are recorded after they complete.
