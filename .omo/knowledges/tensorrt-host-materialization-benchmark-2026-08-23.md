# TensorRT FP16 Host Materialization Benchmark - 2026-08-23

## Decision

- **GO** for replacing the current full-frame FP16 -> FP32 -> RGB chain with direct chunked conversion from the ORT FP16 output view.
- **NO-GO** for pre-bound CPU or CUDA-pinned outputs while the project uses `ort = 2.0.0-rc.11`.
- Keep production code unchanged until the direct conversion is integrated and validated through the full video pipeline.

## Fixed workload

- Fixture: `bench_per_commit_fixture_1080p_120f.mkv`
- Fixture SHA256: `58bfbe8722bd801057aa907ae304bebcc6c2b4bafe979b1770c5e52207210fde`
- Model: `models/the_database_AnimeJaNaiV3L1_sharp_HD_x2_fp16_op17.onnx`
- Model SHA256: `7666ddb0b9078e9fd47bf961e718a67715ad28894477be588d1fe69a9418a0a5`
- Input/output: 1920x1080 RGB -> 3840x2160 RGB, batch 1, FP16 TensorRT
- Hardware: NVIDIA A30-24C
- Runtime: ONNX Runtime 1.23.2, `ort 2.0.0-rc.11`
- TensorRT profile: `input:1x3x1080x1920` for min/opt/max
- Cache: `.omo/benchmarks/videnoa-trt-batch-bench/trt-cache-host-paths-b1/`

## Compared paths

### Current host chain

1. Extract the ORT FP16 output.
2. Materialize an owned FP16 buffer.
3. Convert the full 4K NCHW frame to an approximately 99.5 MB `Vec<f32>`.
4. Quantize and interleave that buffer into a 24.9 MB RGB byte buffer.

### Direct output RGB

1. Borrow the ORT FP16 output view.
2. Read planar FP16 channel values directly.
3. Quantize and interleave immediately into the final 24.9 MB RGB byte buffer.

This removes the owned FP16 output materialization and the full-frame FP32 intermediate.

## Correctness

The direct path was compared byte-for-byte against the current path on the same real model output:

```text
VERIFY strategy=direct_output_rgb bytes=24883200 exact=true
```

All timed checksums were also identical: `2733888`.

## Results

Each round timed 12 complete inference-plus-materialization calls. Strategy order was interleaved to reduce ordering bias.

| Round | Current ms/frame | Current FPS | Direct ms/frame | Direct FPS |
|---:|---:|---:|---:|---:|
| 1 | 296.294 | 3.375 | 123.285 | 8.111 |
| 2 | 296.624 | 3.371 | 123.074 | 8.125 |
| 3 | 297.340 | 3.363 | 123.197 | 8.117 |
| Mean | 296.753 | 3.370 | 123.185 | 8.118 |

Observed effect:

- Frame time reduction: approximately **58.5%**.
- Throughput increase: approximately **140.9%**.
- Throughput multiplier: approximately **2.41x**.
- Variance was low and the result held regardless of which strategy ran first.

The absolute FPS here is a microbenchmark result covering inference plus host output materialization, not end-to-end encoded video throughput. The relative delta is the relevant signal.

## IoBinding blocker

CPU-bound and CUDA-pinned output experiments exposed an ownership defect in the `ort 2.0.0-rc.11` binding path.

Observed CPU-bound lifecycle:

1. `run_binding()` completed.
2. Output synchronization completed.
3. CPU extraction and RGB conversion completed.
4. `SessionOutputs` dropped.
5. `drop(IoBinding)` blocked or faulted.

The rc.11 implementation retains the pre-bound output as an owning `DynValue` in `IoBinding::output_values`. `Session::run_binding_inner()` then wraps the pointer returned by `GetBoundOutputValues` in another owning `DynValue`. Runtime toggle proof confirmed duplicate ownership: intentionally retaining `SessionOutputs` allowed `IoBinding` and `Session` to destruct normally; dropping `SessionOutputs` first reproduced the failure.

CUDA-pinned output has an additional public API blocker: `try_extract_tensor()` rejects `CudaPinned` because rc.11 does not classify it as CPU-accessible.

Leaking outputs or using raw pointers is not an acceptable workaround. Revisit IoBinding only after upgrading ORT and verifying ownership and pinned-memory extraction behavior with an isolated regression harness.

## Production recommendation

Implement only the direct output conversion first:

- Convert directly from the borrowed ORT `f16` output slice to final RGB bytes.
- Preserve the current quantization behavior exactly.
- Avoid changing the internal `Frame::NchwF16` representation as part of the same patch.
- Validate byte equality on deterministic frame output and run the full 120-frame video workflow benchmark.
- Measure end-to-end FPS, inference-stage timing, GPU utilization, output metadata, and encoded frame count before retaining the production change.

Input clone removal remains a separate candidate. Earlier direct-input borrowing regressed throughput, so it should not be bundled with the output materialization change.

## Reproduction

From `.omo/benchmarks/videnoa-trt-batch-bench`:

```bash
export ORT_DYLIB_PATH="$PWD/../../../lib/libonnxruntime.so"
export TRT_LIBS="$HOME/miniconda3/envs/anime/lib/python3.13/site-packages/tensorrt_libs"
export LD_LIBRARY_PATH="$TRT_LIBS:$PWD/../../../lib:$HOME/miniconda3/envs/anime/lib:${LD_LIBRARY_PATH:-}"
export PKG_CONFIG_PATH="$HOME/miniconda3/envs/anime/lib/pkgconfig:${PKG_CONFIG_PATH:-}"

cargo fmt --all -- --check
cargo clippy --release --bin host_paths -- -D warnings
cargo build --release --bin host_paths
./target/release/host_paths \
  ../../../bench_per_commit_fixture_1080p_120f.mkv \
  ../../../models/the_database_AnimeJaNaiV3L1_sharp_HD_x2_fp16_op17.onnx \
  ./trt-cache-host-paths-b1
```

Expected signals:

- `VERIFY strategy=direct_output_rgb ... exact=true`
- Three `HOST_RESULT` lines for each strategy
- Matching checksums across both strategies
- Normal process exit without IoBinding use
