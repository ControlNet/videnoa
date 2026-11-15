# ONNX Runtime and `ort` upgrade assessment (2026-11-15)

Not done yet; the owner postponed it while the GPU machine is busy.

## Where we are

- ONNX Runtime 1.23.2 (Docker `ORT_VERSION`, local `lib/`), TensorRT 10.9,
  CUDA 12.8. Native releases need an external ORT.
- `ort` 2.0.0-rc.12 with `load-dynamic` and `api-23`, no `copy-dylibs` and no
  `download-binaries`.
- ORT keeps older C API versions, so ORT and the `ort` crate can move
  separately. Verify that on the GPU before relying on it.

## `ort` rc.12 -> rc.15

- Gains: rc.15 fixes 30+ undefined-behaviour cases and several leaks (upstream
  says upgrade as soon as possible). rc.13 stops a deadlock when `load-dynamic`
  fails to load the library. Less allocation in `Session::run`.
- Not relevant to us: `run_async`, IoBinding, custom ops, Trainer fixes; the
  Rust 1.100 build break hits only `copy-dylibs`; pyke binaries being CUDA 13
  only.
- rc.15 still has `api-23`, so ORT 1.23.2 keeps working.
- Code changes: rc.14 needs an `Environment` passed to
  `Session::builder(&env)` (two calls in `crates/core/src/nodes/backend.rs`),
  `ep.is_available()` becomes `EP::is_available()`, EP structs are gated by
  Cargo features, `get/set_gpu_device` removed. The four
  `crates/core/examples/spike*.rs` need the same changes (clippy checks
  examples). MSRV 1.92.

## ONNX Runtime 1.23.2 -> 1.32.0

- ORT 1.28+ packages are built with TensorRT 10.14.1.48 (see
  `tools/ci_build/github/azure-pipelines/templates/common-variables.yml` at the
  release tag), so TensorRT has to move from 10.9 to 10.14 with it. Every
  cached TensorRT engine is rebuilt (8-12 min per model and shape).
- Possible gains: TensorRT 10.14 engines (benchmark ESRGAN/RIFE before and
  after); since 1.28 cuDNN and cuFFT are optional at runtime for the CUDA EP
  and nvrtc is not linked (smaller image if everything runs on TensorRT); 1.32
  speeds up session init on large graphs and caches cuDNN Conv plans; a year of
  core and TensorRT EP fixes.
- Most 1.24-1.32 headline features (MoE, LLM attention, WebGPU, plugin EPs) do
  not apply.
- CUDA 12 packages are deprecated since 1.27 but still published as
  `onnxruntime-<os>-x64-gpu_cuda12-<ver>`. CUDA 13 needs driver >= 580.
- The CUDA EP is moving to a separately released plugin EP. Check that the
  `gpu_cuda12` 1.32 archive still ships `libonnxruntime_providers_tensorrt.so`
  and `..._cuda.so` before planning around it.

## Suggested order

1. Locally: ORT 1.32 `gpu_cuda12` + TensorRT 10.14 + `ort` rc.15, CUDA 12.8
   unchanged. Compare speed and output on the RIFE + AnimeJaNai workflow.
2. Update the Docker image and release only if step 1 shows a gain or fixes a
   real problem. Move to CUDA 13 when CUDA 12 packages stop.
