# Videnoa v0.1.3 Release Evidence

Date: 2026-09-01 (Australia/Melbourne)

## Release identity

- Release: https://github.com/ControlNet/videnoa/releases/tag/v0.1.3
- Tag `v0.1.3` resolves to merge commit `5560738ca4891af092f4e39b7e638a9a9e02ed5a`.
- Release workflow: https://github.com/ControlNet/videnoa/actions/runs/33427349116
- The complete release workflow passed, including Linux and Windows packages, Docker publication, GitHub Release publication, and release outcome verification.
- `master` was merged back into `dev` as `a97e46de8b2cc70c02317ee75c42acee55c3cf58`.

## Public release assets

- `videnoa-linux64-0.1.3.7z.001`: 2,097,152,000 bytes, SHA-256 `cbb7779fe708b6964268a2d32770a840b89608a0ffc6cf66843176831853ba0f`
- `videnoa-linux64-0.1.3.7z.002`: 192,277,700 bytes, SHA-256 `be9b1d3ce96b392f9aa85b722d2b10d93e187138377cc2fe62f6aec900e8d64d`
- `videnoa-win64-0.1.3.7z.001`: 1,711,973,608 bytes, SHA-256 `af925fe79f13e2266b7bd3caef72af70bbac6a96d6cc037cd7e05d3fc701dcbb`
- Both public archives passed `7z t`.
- The Linux archive contains 38 files under a single `videnoa/` root.
- The Windows archive contains 30 files under a single `videnoa/` root, including `videnoa.exe` and `videnoa-desktop.exe`.
- The downloaded Windows CLI and desktop executables are x86-64 PE32+ binaries.

## Linux compatibility verification

- The public Linux archive was downloaded and extracted on Ubuntu 22.04.5 LTS.
- `videnoa --help` started successfully from the extracted bundle.
- `scripts/check_linux_package_compat.sh` passed against the extracted bundle.
- Both packaged executables are x86-64 ELF binaries.
- The highest GLIBC symbol requirement observed across `videnoa` and `videnoa-desktop` was `GLIBC_2.34`, within the release ceiling of `GLIBC_2.35`.
- All eight required cuDNN 9 runtime libraries were present:
  - `libcudnn.so.9`
  - `libcudnn_adv.so.9`
  - `libcudnn_cnn.so.9`
  - `libcudnn_engines_precompiled.so.9`
  - `libcudnn_engines_runtime_compiled.so.9`
  - `libcudnn_graph.so.9`
  - `libcudnn_heuristic.so.9`
  - `libcudnn_ops.so.9`

## Docker verification

- Published image: `controlnet/videnoa:0.1.3`
- Pulled digest: `sha256:5d95ab4a62716c7d8816bdfb560dfa9ac4cb3aafc86b8d70deee798f5e768ddf`
- `docker run --rm controlnet/videnoa:0.1.3 videnoa --help` exited successfully and rendered the CLI help.
- The image configuration uses `Cmd=["videnoa"]` and has no explicit entrypoint.

## Development branch synchronization

- `master` was merged into `dev` with merge commit `a97e46de8b2cc70c02317ee75c42acee55c3cf58` and pushed to `origin/dev`.
- Dev workflow: https://github.com/ControlNet/videnoa/actions/runs/33434293249
- The complete `dev` workflow passed, including Rust tests on Linux and Windows, web builds on Linux and Windows, Docker CLI smoke, and Linux and Windows package smoke tests.

## Non-blocking observations

- GitHub Actions reported that several actions still target deprecated Node.js 20 runtimes and are being forced onto Node.js 24.
- A tracked-file secret scan reported a false positive in `crates/core/src/logging.rs:841` for a test string containing `token=`; no release evidence file was staged or committed.


## September 7 remote-state correction

Before preparing the current v0.1.3 release, GitHub listed v0.1.2 as Latest and
`git ls-remote --tags origin` contained no v0.1.3. Treat the above as historical
records, not proof of current availability. Current release evidence is recorded
in `release-v0.1.3-2026-09-07.md`; no existing tag is being replaced.
