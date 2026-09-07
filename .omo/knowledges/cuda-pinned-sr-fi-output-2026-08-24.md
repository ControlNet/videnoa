# CUDA-Pinned SR/FI Output Validation (2026-08-24)

## Decision

- Dynamic ONNX Runtime outputs use `IoBinding::bind_output_to_device()`.
- A concrete `Session` selects `CudaPinned/CPUOutput` only when
  `Allocator::new(session, pinned_memory_info)` succeeds.
- Sessions without that allocator fall back to `Cpu/Default`.
- Every bound output is checked with `ensure_inference_output_memory()` before
  its tensor data is extracted.
- `SessionBuilder::with_allocator` is not used for this purpose because it
  controls value metadata allocation, not tensor-content allocation.

## Production Coverage

- Super resolution: FP32 IoBinding, FP16 micro-stage, converted FP16,
  full-frame FP16, and tiled FP16 paths.
- Frame interpolation: three-input and concatenated-input IoBinding paths.

## Runtime Contract

- Crate: `ort = 2.0.0-rc.12` with API 23.
- Runtime: ONNX Runtime 1.23.2.
- GPU output: allocation device `CudaPinned`, memory type `CPUOutput`.
- CPU-only output: allocation device `Cpu`, memory type `Default`.
- Do not enable API 24 or mix runtime/provider versions.

## Correctness Fixtures

- Input: `bench_per_commit_fixture_1080p_120f.mkv`.
- Input SHA256:
  `58bfbe8722bd801057aa907ae304bebcc6c2b4bafe979b1770c5e52207210fde`.
- SR decoded framemd5 SHA256:
  `6b68085e0dde975988bb7e978c4d2ea461d8b2df4ec7ab5ec2a6d33c49bedb2b`.
- FI decoded framemd5 SHA256:
  `a739a306cb0ab27d2b65152b87cbbda07395eae3a88f1b9760df3d971bfbb008`.

The final GPU smoke run logged `CudaPinned/CPUOutput` for both pipelines.
Its SR output was 3840x2160, HEVC, `yuv420p10le`, 2997/125. Its FI output
was 1920x1080, HEVC, `yuv420p10le`, 5994/125.

## A/B Results

The frozen pre-change binary was
`/tmp/opencode/cuda-pinned-sr-fi/videnoa-default`, SHA256
`218865e4630c13cac08717fb2ac0f4972b360b4696757a0e0adc8a83e4f87918`.
Runs used warmed TensorRT caches and interleaved baseline/candidate ordering.

- SR was bit-exact. Median inference time improved from about 64.65 ms to
  59.3 ms (about 8.3%). Median wall time was effectively flat at about
  14.73 s versus 14.69 s. Peak GPU memory remained 1653 MiB, RSS stayed
  within the 5% gate, and swap remained zero.
- FI was bit-exact. Interpolation-stage time improved about 1.5%, and wall
  time improved about 1.6%. Peak GPU memory remained 1946 MiB, RSS increased
  about 0.3%, and swap remained zero.

## Verification

The final change passed:

```bash
cargo check --workspace --all-targets
cargo test --workspace
cargo build --release --workspace
cargo fmt -- crates/core/src/nodes/backend.rs \
  crates/core/src/nodes/super_res.rs \
  crates/core/src/nodes/frame_interpolation.rs -- --check
```

The CPU-only integration test was run explicitly despite being ignored by
default. LSP diagnostics and `git diff --check` were clean for all three
changed files. Full `cargo fmt --all -- --check` remains blocked by unrelated,
pre-existing formatting changes in `crates/app/src/lib.rs`.

## Review

Oracle approved the final implementation with no blocker. The temporary
allocator probe and `MemoryInfo` lifetimes match ORT rc.12 ownership rules.
The only non-blocking observation is that allocator availability is probed on
each inference (and each tile); cache it per session only if profiling proves
the wrapper creation is material.
