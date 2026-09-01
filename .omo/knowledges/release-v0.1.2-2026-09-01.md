# v0.1.2 release evidence

## Release scope

- Canonical version source: root `Cargo.toml` at `[workspace.package].version`.
- Release branch: `release/0.1.2` from `dev`.
- Linux packaging now merges the versioned `cudnn_linux64_9.14.zip` misc asset into `videnoa/lib/`.
- The packaging script validates all eight required cuDNN 9 split libraries before completing a Linux bundle.
- Unix runtime preloading follows dependency tiers: CUDA, cuDNN core/graph, cuDNN engines/heuristic/ops, cuDNN adv/cnn, then TensorRT.
- An explicit `ORT_DYLIB_PATH` owns its complete dependency stack and skips package-discovered GPU library preloading.
- The release workflow must create and push `v0.1.2`; do not push the tag before the `master` release workflow runs.

## Root cause and fix evidence

- The prior Linux misc archive included only `libcudnn.so.9` and `libcudnn_graph.so.9`. cuDNN 9.14 dynamically loads six additional split libraries during the first convolution.
- The original packaged CUDA run failed with `CUDNN_STATUS_SUBLIBRARY_LOADING_FAILED`.
- Adding all six missing split libraries changed the same workflow from exit 1 to exit 0. Removing only `libcudnn_engines_precompiled.so.9` restored failure.
- Equal-priority alphabetical cuDNN preloading could load dependent libraries before their prerequisites. The old package produced all 239 frames and then aborted with `corrupted double-linked list`.
- A red-first runtime test proved the missing dependency tiers. The ordered loader eliminated the teardown abort in repeated package-level CUDA and TensorRT runs.
- Oracle identified that explicit `ORT_DYLIB_PATH` still mixed with auto-discovered package libraries. A red-first ownership test and runtime guard isolated caller-owned runtime stacks.
- Final Oracle review verdict: PASS, with no P0 or P1 release blockers.

## Local quality gates

- `cargo test -p videnoa-core --lib --tests`: 566 tests passed.
- Crash-hook integration coverage: 3 tests passed.
- `cargo test -p videnoa-app --lib --tests`: 22 tests passed.
- `cargo clippy -p videnoa-core --all-targets --all-features -- -D warnings`: passed.
- `cargo fmt --all -- --check`: passed.
- `bash -n scripts/package_dist.sh`: passed.
- `git diff --check`: passed.
- Web `npm ci`, `npm run lint`, `npm test -- --run`, and `npm run build`: passed.
- rust-analyzer reported no errors or warnings in changed Rust source; Windows-only inactive `cfg` hints remain informational.
- Full workspace Clippy remains blocked by pre-existing `clippy::type_complexity` findings in `crates/app/src/lib.rs`; strict Clippy for the changed core crate passes.

## Packaged Linux manual QA

- Environment: NVIDIA A30-24C, driver `580.65.06`.
- Final bundle: `/tmp/opencode/videnoa-release-0.1.2-linux-ordered/videnoa`.
- CUDA, one worker: exit 0, 239 frames, 1920x1080, no allocator or cuDNN errors.
- CUDA, two workers: exit 0, 239 frames, no teardown error.
- TensorRT cold cache: exit 0, 239 frames, 14 MB SM80 engine/profile cache created.
- TensorRT warm cache: exit 0, 239 frames, existing engine/profile cache reused without file changes.
- TensorRT super-resolution then frame interpolation: exit 0, 3840x2160, 239 frames, cache expanded to 28 MB.
- Preview API: returned three frame URLs; the first was a valid 1920x1080 RGB PNG. `count=0` returned HTTP 400 with `count must be between 1 and 100`.
- Automatic bundled runtime discovery: exit 0, 239 frames.
- Explicit repository `ORT_DYLIB_PATH` plus caller-owned CUDA/TensorRT `LD_LIBRARY_PATH`: exit 0, 239 frames.

## Misc asset

- URL: `https://github.com/ControlNet/videnoa/releases/download/misc/cudnn_linux64_9.14.zip`
- SHA-256: `e294dbc6e2fc901f36e8cba027e854fcf77fcb89390c46724511eb25d17d8834`

## Remote release verification

- GitHub Release `v0.1.2` is public and marked Latest: https://github.com/ControlNet/videnoa/releases/tag/v0.1.2
- Annotated tag `v0.1.2` resolves to commit `5dcc10aaa86bcd3284b7c66806a8a7524249c4ad`.
- Release workflow https://github.com/ControlNet/videnoa/actions/runs/33451583500 passed all jobs: Linux and Windows Rust tests/web builds, Linux and Windows package smoke, Docker build smoke, Linux and Windows release packages, Docker publication, GitHub Release publication, and outcome verification.
- Public Linux assets match the GitHub API digests: `.001` SHA-256 `dfb92890cb0e65f9a4cdf6e583331e617b873cde8c94e5409c1c12a294484c58`; `.002` SHA-256 `298899ce86031efad5449914deef934e2faf9ddd162775852d1a9ef12b6aa6f0`.
- The public Linux split archive passed `7z t`: 38 files, 5 folders, 5,330,656,318 uncompressed bytes under one `videnoa/` root.
- The extracted public Linux package passed `scripts/check_linux_package_compat.sh`, including CLI startup and the GLIBC 2.35 ceiling.
- From the extracted public package, the server loaded the bundled ORT library and embedded frontend, stayed running, and returned `200 {"status":"ok"}` from `/api/health`.
- From the extracted public package, a CUDA RIFE workflow processed a synthetic 64x64, 4-frame H.264 fixture successfully. The HEVC output decoded without errors and contained 7 frames at 4 fps in `yuv420p10le`.
- Docker tag `controlnet/videnoa:0.1.2` is public at digest `sha256:b6572ba0d3d82b8142bb4cdd489c1f4fd81ae8ee4229ab86cc71b9f25ba127be`.
- The Docker image passed CLI startup, NVIDIA A30 GPU passthrough, server health/embedded frontend/API model-list checks, and the same CUDA RIFE workflow. Its output decoded without errors and contained 7 frames at 4 fps in `yuv420p10le`.
- Release CI itself only runs `videnoa --help` for Docker and package compatibility/layout checks; the server-health and real CUDA inference checks above were additional manual verification on 2026-09-01.
- Current `dev` CI is also passing; the release validation does not depend on later `dev` commits.

## Known non-blocking risks

- Packaging validates expected cuDNN filenames and package-level behavior, but does not independently validate every ELF SONAME or dependency edge.
- Native preload failures are currently silent, which can reduce diagnostics on incompatible hosts.
- Windows package behavior is covered by CI build and smoke checks; the Linux runtime ownership change received the package-level GPU validation.
- `npm ci` reports 16 dependency vulnerabilities.
- GitHub Actions warns about deprecated Node.js 20 action runtimes.
- The production web bundle remains approximately 1.22 MB.
