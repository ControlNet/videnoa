# Docker Image Slimming (2026-09-10)

## Implemented low-risk reductions

- The repository-root `.dockerignore` excludes local build outputs, runtime data,
  model/runtime libraries, media, frontend dependency/build directories, agent
  state, and common secret files from both Docker build contexts.
- The Worker runtime no longer copies `/app/web/dist`; release builds serve the
  same frontend from the assets embedded by `rust-embed`.
- The Worker keeps `mkvpropedit` but removes the unused `mkvextract`, `mkvinfo`,
  and `mkvmerge` executables in the same layer that installs MKVToolNix.

## Symbol retention decision

- Stripping was evaluated and then reverted. It saved about 13 MB of unpacked
  size per binary but only about 1.7 MB after gzip compression.
- Release binaries retain their symbol tables so production panic backtraces,
  core dumps, and offline address symbolization remain useful.
- The source contract rejects future `strip` commands in either Dockerfile.
- The retained-symbol QA images measured 5,815,081,126 bytes for the Worker and
  133,269,696 bytes for the Controller. The remaining Worker changes remove
  about 20 MB of unpacked payload from the duplicate WebUI and unused MKV tools.

## Measured build context

- The Docker build context observed during the Worker build was 4.54 MB. The
  Controller build transferred a 45.17 kB incremental context.

## Verification contract

`scripts/tests/docker_slimming_contract_test.sh` validates the source contract.
When passed a Worker image tag, it also verifies the runtime filesystem and starts
the server without a GPU to prove that health and the embedded WebUI remain
available. The Controller continues to use
`scripts/check_controller_container.sh <image> --all` for its full contract.

## CUDA runtime reduction

- The Worker runtime now starts from
  `nvidia/cuda:12.8.0-base-ubuntu22.04` and installs only the pinned CUDA 12.8
  libraries required by the bundled ONNX Runtime CUDA/TensorRT providers:
  CUDA runtime, NVRTC, cuBLAS/cuBLASLt, cuFFT, cuRAND, cuDNN, nvFatBin, and
  nvJitLink.
- The change omits cuSolver, cuSPARSE, NCCL, NPP, cuFile, nvJPEG, and the CUDA
  OpenCL package. These packages are absent from the resulting image and are
  not direct ELF dependencies of the bundled providers.
- The exact before/after images were built from the same retained-symbol source.
  Unpacked image size changed from 5,815,081,126 to 4,765,685,963 bytes: a
  reduction of 1,049,395,163 bytes (about 1.05 GB, 18.05%). A same-host
  `docker image save | pigz -1` comparison changed from 3,709,293,132 to
  2,956,278,630 bytes, a 753,014,502-byte (20.30%) compressed-transfer proxy;
  this is not a registry-manifest measurement.
- The source/image contract now verifies the minimal base, explicit retained
  package set, omitted package set, and resolved ELF dependencies for both GPU
  providers.

## Release runtime alignment

- The Worker Docker runtime matches the published release environment at the
  component-version level: CUDA runtime 12.8.57, NVRTC 12.8.61, cuBLAS
  12.8.3.14, cuFFT 11.3.3.41, cuRAND 10.3.9.55, nvFatBin 12.8.55, nvJitLink
  12.8.61, cuDNN 9.14.0.64, TensorRT 10.9.0.34, and ONNX Runtime 1.23.2.
- The cuDNN, TensorRT, and ONNX Runtime core/provider libraries in the image
  are byte-identical to the corresponding published-release bundle files.
  CUDA packages use NVIDIA's Ubuntu packages rather than the release build's
  conda packages, but their upstream component versions are pinned exactly.
- The aligned image is 6,444,257,914 bytes unpacked versus 4,765,685,963 bytes
  for the CUDA 12.6/TensorRT 10.7 slim image. Most of the 1,678,571,951-byte
  increase comes from TensorRT 10.9's required builder resource, which is kept
  so a clean installation can compile TensorRT engines.

## CUDA/TensorRT manual QA

- Hardware: NVIDIA A40-24Q, compute capability 8.6, driver 580.65.06.
- A real one-second 1280x720 clip derived from `test-30s.mkv` completed CUDA RIFE
  inference in both the full baseline and slim image with exit code zero. Both
  outputs contain 59 HEVC 10-bit frames at 60 fps and 0.984 seconds. Baseline
  run-to-run decoded PSNR was 51.54 dB; baseline-to-slim was 51.34 dB, so the
  observed CUDA nondeterminism did not increase materially after slimming.
- An empty-cache TensorRT super-resolution run compiled an SM86 engine/profile
  pair (407,942 bytes) and completed 30 real frames. A warm rerun detected both
  files, reused them without size or timestamp changes, and produced a
  byte-identical decoded frame-hash stream.
- Direct FFmpeg `h264_cuvid` decode through `h264_nvenc` encode completed with
  exit code zero when NVIDIA video driver capabilities were mounted.
- With TensorRT 10.9, RIFE v4.26 now cold-compiles successfully from an empty
  cache at 1920x1080 on SM86. The 24-frame real fixture produced 47 frames. A
  warm rerun reused the 13,966,230-byte engine/profile cache without changing
  its size or timestamps and completed the same workflow in about seven seconds.
- A TensorRT 10.7 super-resolution cache does not prevent upgrading. TensorRT
  10.9 creates a separate engine beside the old engine on the first run, then
  reuses the new engine without modifying the cache on the next run.
- Direct FFmpeg `h264_cuvid` decode through `h264_nvenc` encode still completes
  successfully with the aligned runtime.
- CUDA RIFE processes and writes the expected 47-frame output with the aligned
  runtime, but then reports `corrupted double-linked list` while tearing down
  and exits non-zero. The same post-completion failure reproduces outside Docker
  with the published release binary and release dependency environment, with
  both one and two inference workers. It is therefore an existing release-path
  teardown defect rather than a Docker alignment mismatch.
