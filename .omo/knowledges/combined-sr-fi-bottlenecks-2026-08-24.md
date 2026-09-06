# Combined SR/FI Bottleneck Analysis (2026-08-24)

## Scope

Production presets:

- `presets/anime-2x-interpolation-2x.json` (`SR -> FI`)
- `presets/interpolation-2x-anime-2x.json` (`FI -> SR`)

The diagnostic fixture was a deterministic 24-frame stream copy from
`bench_per_commit_fixture_1080p_120f.mkv`: 1920x1080, 24000/1001 FPS.
TensorRT caches were already warm for the shipped 1080p SR and FI profiles.

## Executive Finding

- `SR -> FI` is cache-state dependent and therefore not a reliable TensorRT
  production path for this 1080p input and 2x SR preset. RIFE receives
  3840x2160 tensors but production points both sessions at the shared root
  `trt_cache/`. If that root contains the fixed 1080p RIFE engine, the first FI
  pair fails when TensorRT cannot set input `input` to the 4K shape.
- The released `master` / `v0.1.0` build at `870d153` reproduces the same failure
  with the warmed 1080p cache. Its first FI pair attempted
  `[1,7,2176,3840]` against an engine expecting `[1,7,1088,1920]`.
- The same released binary succeeds with an isolated empty cache: TensorRT builds
  a new 4K RIFE engine, then 8 input frames produce 15 HEVC frames at 3840x2160,
  5994/125 FPS, `yuv420p10le`. This confirms a pre-existing cache/profile
  isolation defect rather than a regression introduced by the dev ORT upgrade
  or CUDA-pinned output changes.
- Switching only FI to CUDA EP did not provide a viable fallback: the first 4K
  pair failed while building a cuDNN frontend convolution graph. RSS had already
  reached about 6.26 GiB and device memory about 2.74 GiB.
- `FI -> SR` runs correctly. For 24 source frames it produced 47 frames at
  3840x2160, 5994/125 FPS, HEVC, `yuv420p10le`. Two timed outputs had identical
  decoded framemd5 SHA256:
  `0446f75f41f06bb376abda217dcca4d0f27c1f3abdc22dad38391ecfc0064a78`.

## Cache/Profile Defect

`backend::trt_cache_key()` and `resolve_trt_cache_dir()` can construct a key
containing compute capability, model hash, and input resolution. Production does
not use them. `VideoCompileContext` clones one `trt_cache_dir` into both SR and FI,
and `build_session()` gives that root directly to TensorRT.

Consequences:

1. A previously cached 1080p RIFE engine is reused for a 4K RIFE invocation.
2. Different model/resolution profiles share one cache namespace.
3. Failure occurs during inference rather than during workflow validation.
4. A clean cache can make the same workflow succeed, so behavior depends on
   which resolution populated the shared cache first.

This must be fixed before `SR -> FI` performance work is meaningful. Cache
directories should be model-, device-, and shape-specific, or the FI TensorRT
profile must explicitly cover all intended dynamic shapes.

## FI -> SR Measurements

Three warmed 24-input-frame runs:

| Metric | Warm | Hot 1 | Hot 2 | Median |
|---|---:|---:|---:|---:|
| Wall time | 11.22 s | 10.95 s | 10.87 s | 10.95 s |
| Output frames | 47 | 47 | 47 | 47 |
| Output end-to-end FPS | 4.19 | 4.29 | 4.32 | 4.29 |
| Input-equivalent FPS | 2.14 | 2.19 | 2.21 | 2.19 |
| Max RSS | 5880 MiB | 5888 MiB | 5867 MiB | 5880 MiB |

Median stage costs:

| Stage | Unit | Median cost | Interpretation |
|---|---|---:|---|
| FI inference | source pair | 69.5 ms | About 34.8 ms per emitted output-frame equivalent |
| FI preprocess | source frame | 17.5 ms | Mostly blocked downstream after its CPU conversion |
| FI postprocess | output frame | 9.3 ms | Tensor pass-through/crop and queue pressure |
| SR preprocess | output frame | 13.9 ms | FP32-to-FP16 conversion; mostly blocked on SR |
| SR inference/direct RGB | output frame | 58.9 ms | Primary compute/host-conversion limiter |
| x265 encode | output frame | 52.5 ms | Close secondary limiter |

The pipeline overlaps stages, so these costs must not be added. The throughput
limit is set by the slowest sustained stage. SR inference/direct-RGB is slightly
slower than x265 encoding; run-to-run encoding ranged from 49.8 to 62.9 ms/frame,
so the two stages form a near-balanced dual bottleneck.

GPU telemetry from the clean hot run (200 ms samples):

- Active-sample mean GPU utilization: 9.47%.
- Active-sample p95 GPU utilization: 28.5%.
- Mean memory-controller utilization: 3.84%.
- Peak device memory: 2296 MiB (whole device; idle was about 694 MiB).

Low GPU occupancy while SR is the longest stage indicates that the measured SR
stage includes substantial host-side conversion, transfers, synchronization,
and launch gaps. It is not a saturated GPU-kernel workload.

## Ordering-Specific Bottlenecks

### SR -> FI

1. **TensorRT shape/cache correctness (blocking):** 4K RIFE cannot use the cached
   1080p engine.
2. **4K RIFE feasibility and memory:** CUDA EP also failed on the first 4K pair;
   FI preprocessing was already about 320-336 ms/frame before failure.
3. **FP16-to-FP32 4K handoff:** SR emits owned `NchwF16`; FI converts and copies it
   into owned padded `NchwF32`. Pinned ORT output does not make the inter-stage
   `Frame` GPU-resident.
4. **Seven-channel 4K RIFE input:** concatenated FI builds `[1,7,H,W]` FP32 input,
   about 232 MiB at 3840x2160 before additional tensors/workspace.
5. **Final 4K tensor-to-RGB and x265:** if 4K FI becomes runnable, 47 final 4K
   frames still require CPU RGB materialization and software encoding.

### FI -> SR

1. **SR invocation multiplication:** FI expands 24 source frames to 47 frames;
   SR therefore runs 47 times instead of 24.
2. **SR inference/direct-RGB stage:** median 58.9 ms per final frame and the
   strongest stable bottleneck.
3. **4K libx265 encoding:** median 52.5 ms per frame and occasionally slower than
   SR, so optimizing SR alone will quickly move the limit to encoding.
4. **Host materialization remains:** CUDA-pinned ORT outputs are copied into owned
   ndarray/Vec frames. FI outputs `NchwF32`, SR preprocess converts them to FP16,
   and terminal SR converts FP16 output to owned RGB.
5. **Memory/backpressure:** median RSS is about 5.88 GiB. Capacity-four channels
   retain whole owned frames; upstream send waits show queues filling behind SR
   and encoding.
6. **CPU decode:** production explicitly disables hardware decode. It is hidden by
   slower downstream stages in this test, so it is lower priority today.

## Optimization Priority

1. Fix TensorRT cache/profile isolation and fail early on unsupported shapes.
2. Establish a valid 4K RIFE configuration (dynamic profile, tiled FI, or an
   explicit resolution policy) before benchmarking `SR -> FI`.
3. For `FI -> SR`, isolate SR ORT execution from direct FP16-to-RGB timing. The
   low GPU utilization suggests host work can still be reduced or overlapped.
4. Benchmark the same workflow with a null or faster encoder. If SR stays near
   59 ms/frame, optimize SR output conversion/transfer; if wall time falls to the
   SR limit, move encoding to a faster preset or hardware encoder.
5. Replace owned CPU tensor handoffs with reusable/pinned pools or a real device
   tensor representation only after the stage-specific measurements justify the
   added ownership complexity.
6. Reduce queue memory adaptively for 4K owned frames; capacity four at every
   micro-stage is expensive and does not improve throughput when downstream is
   already saturated.

## Reproduction Commands

```bash
export ORT_DYLIB_PATH=$PWD/lib/libonnxruntime.so
export TRT_LIBS=$HOME/miniconda3/envs/anime/lib/python3.13/site-packages/tensorrt_libs
export LD_LIBRARY_PATH=$TRT_LIBS:$PWD/lib:$HOME/miniconda3/envs/anime/lib:$LD_LIBRARY_PATH
export PKG_CONFIG_PATH=$HOME/miniconda3/envs/anime/lib/pkgconfig:$PKG_CONFIG_PATH

./target/release/videnoa run presets/anime-2x-interpolation-2x.json \
  --input <1080p-input> --output <sr-fi-output>

./target/release/videnoa run presets/interpolation-2x-anime-2x.json \
  --input <1080p-input> --output <fi-sr-output>
```

Expected current signals:

- `SR -> FI` now isolates TensorRT caches by device, ONNX model SHA-256, aligned
  input dimensions, and (for tiled SR) tile size plus original dimensions.
- A warmed 1080p RIFE namespace is not reused for 4K input. The 4K invocation
  creates or reuses its own `2176x3840` namespace instead of failing at
  `setInputShape()`.
- `FI -> SR`: 2x input frame-rate output at 2x dimensions; SR and encoder stage
  costs near 50-60 ms per final frame.

## Implemented Fix and Verification

The production cache namespace is now resolved before TensorRT session creation.
`TrtCacheIdentity` includes the device ID and aligned input dimensions, while
`TrtTileIdentity` additionally includes tile size and original frame dimensions
so distinct edge-tile shape regimes cannot collide. Model identity is the SHA-256
of the ONNX file contents.

The resulting key format is:

```text
device-{device_id}_{model_sha256}_{input_h}x{input_w}[_tile-{tile_size}_orig-{original_h}x{original_w}]
```

`VideoCompileContext` skips duplicate graph-time execution of SR and FI after it
has initialized their processing stages with the resolved namespace. Other
compile contexts retain the previous execution behavior, and CUDA execution
returns before model hashing or tile-identity work.

Regression coverage verifies isolation when the model, device, aligned shape,
tile size, or original tiled dimensions change, including RIFE 1080p
`1088x1920` versus 4K `2176x3840` and original tiled heights 66 versus 68 that
share an aligned height but produce different edge tiles.

A real warmed-cache CLI run used an 8-frame 1920x1080 input at 24000/1001 FPS
with `presets/anime-2x-interpolation-2x.json`. The cache root was seeded with the
existing SR namespace and a warmed 1080p RIFE namespace. The run retained those
namespaces, created a distinct 4K RIFE namespace, logged `Workflow completed
successfully`, and produced 15 HEVC frames at 3840x2160, 5994/125 FPS,
`yuv420p10le`, with no TensorRT shape error.

Final review found no blocking correctness or scope issue. Formatting, the core
test suite, the release workspace build, changed-file diagnostics, and the Rust
no-excuse checker passed. Strict workspace clippy remains blocked by unrelated
pre-existing warnings outside the changed files.

## Follow-up Performance Improvement (2026-08-25)

With cache isolation fixed and the 4K RIFE engine warm, the `SR -> FI` path was
measured on a deterministic 24-frame 1080p fixture. `FrameInterpolationInference`
borrowed two `Frame::NchwF32` values but `extract_nchw_f32()` cloned both padded
4K tensors before reshaping them. Each clone was approximately 100 MiB, and both
were copied again into the reusable seven-channel RIFE input.

The fix returns borrowed slices from `extract_nchw_f32()`, reshapes them as
`ArrayView4`, and copies directly into the existing concatenation buffer. A
regression test verifies that extraction retains the frame's backing pointer.

Three warmed runs before and after the change produced:

| Metric | Baseline median | Candidate median | Change |
|---|---:|---:|---:|
| Wall time | 19.20 s | 16.72 s | 12.9% faster |
| FI inference | 484.3 ms/pair | 377.5 ms/pair | 22.1% lower |
| Peak RSS | 7,437,640 KiB | 7,110,224 KiB | 4.4% lower |

All six decoded outputs had the same framemd5 SHA-256,
`419d212c001242c651defb2ec7edec8adc6f768868ba3b49e05539049892b896`.
The candidate output remained 47 frames, HEVC Main 10, 3840x2160,
`yuv420p10le`, at `5994/125` FPS.

## Second FI Ownership Improvement (2026-08-25)

The next measured FI preprocess allocation was the final
`padded.as_slice().to_vec()` in the `Frame::NchwF16` branch. The padded 4K
tensor contains 25,067,520 FP32 values, so this copied 95.625 MiB for every
source frame even though preprocessing already owned the `Array4<f32>` and the
next stage only needed an owned `Vec<f32>`.

The fix validates standard layout and consumes the array with
`into_raw_vec_and_offset()`. A RED-first 30x50 padding test verifies that the
returned vector retains the array's backing pointer, padded element count, and
channel value. The CpuRgb reusable-buffer path is unchanged.

Three warmed runs before and after the change produced:

| Metric | Baseline median | Candidate median | Change |
|---|---:|---:|---:|
| Wall time | 17.44 s | 16.41 s | 5.9% faster |
| FI preprocess | 307.7 ms/frame | 193.8 ms/frame | 37.0% lower |
| Peak RSS | 7,185,744 KiB | 7,222,640 KiB | 0.5% higher |

Every candidate and manual-QA output retained decoded framemd5 SHA-256
`419d212c001242c651defb2ec7edec8adc6f768868ba3b49e05539049892b896`
and the 47-frame 3840x2160 HEVC Main10 output contract. Oracle found no blocking
ownership, correctness, or benchmark-evidence issue.

## Third FI Ownership Improvement (2026-08-25)

The next NchwF16 FI preprocess allocation occurred before padding. SIMD
conversion produced an owned 4K `Vec<f32>`, then the shared borrowed-F32 reshape
helper cloned it with `data.to_vec()`. This duplicated 99,532,800 bytes
(94.922 MiB) per source frame.

The fix introduces an owned-Vec reshape boundary. The borrowed NchwF32 path
retains its explicit copy, while NchwF16 moves its newly converted Vec directly
into `Array4::from_shape_vec`. A RED-first pointer test locks this ownership
transfer without changing padding, error text, or output semantics.

| Metric | Fresh baseline median | Candidate median | Change |
|---|---:|---:|---:|
| Wall time | 15.81 s | 15.07 s | 4.68% faster |
| FI preprocess | 204.5 ms/frame | 183.4 ms/frame | 10.32% lower |
| Peak RSS | 7,298,404 KiB | 7,170,944 KiB | 1.75% lower |

All candidate and manual-QA outputs retained decoded framemd5 SHA-256
`419d212c001242c651defb2ec7edec8adc6f768868ba3b49e05539049892b896`
and the 47-frame 3840x2160 HEVC Main10 output contract. Oracle approved the
increment without blockers.

## Rejected Direct-Padding Experiment (2026-08-25)

After the third accepted improvement, FI preprocessing still allocated an
unpadded 94.922 MiB FP32 conversion buffer and then copied its logical region
into the required 95.625 MiB reflection-padded tensor. A RED-first experiment
converted FP16 rows directly into the final padded allocation and completed
right/bottom reflection there. Default and strict-provenance Miri, focused
tests, all 550 core tests, and the release workspace build passed.

Interleaved frozen-baseline/candidate runs produced:

| Metric | Baseline median | Candidate median | Change |
|---|---:|---:|---:|
| Wall time | 16.32 s | 16.12 s | 1.23% faster |
| FI preprocess | 189.9 ms/frame | 118.7 ms/frame | 37.49% lower |
| Peak RSS | 7,176,340 KiB | 7,156,724 KiB | 0.27% lower |

All six decoded outputs retained framemd5 SHA-256
`419d212c001242c651defb2ec7edec8adc6f768868ba3b49e05539049892b896`.
The implementation was reverted because the workflow wall-time improvement did
not meet the predeclared 2% acceptance gate. This confirms that FI inference
variance and queue backpressure now dominate enough to hide a large local
preprocessing gain; future work should target the inference stage or ownership
across stage boundaries rather than further isolated preprocessing copies.

A later reconsideration replaced the fixed 2% threshold with a stability gate:
five fresh alternating-order pairs, at least four candidate wall-time wins, and
a positive median paired improvement. The baseline binary was rebuilt from
`519d661` with the candidate's exact ignored `Cargo.lock` to keep dependency
resolution identical.

| Pair | Execution order | Baseline wall | Candidate wall | Candidate result |
|---:|---|---:|---:|---|
| 1 | baseline, candidate | 14.63 s | 16.11 s | 1.48 s slower |
| 2 | candidate, baseline | 15.98 s | 14.90 s | 1.08 s faster |
| 3 | baseline, candidate | 15.04 s | 15.76 s | 0.72 s slower |
| 4 | candidate, baseline | 14.68 s | 14.60 s | 0.08 s faster |
| 5 | baseline, candidate | 15.52 s | 16.03 s | 0.51 s slower |

The candidate won only two of five pairs, and the median paired wall difference
was a 0.51-second regression. Its standalone wall median was 15.76 seconds
versus 15.04 seconds for baseline. FI preprocess improved in every pair, with
medians of 177.1 versus 62.2 ms/frame, while median RSS increased only 0.65%
from 7,158,312 to 7,205,140 KiB. All ten decoded outputs retained framemd5
SHA-256 `419d212c001242c651defb2ec7edec8adc6f768868ba3b49e05539049892b896`
and identical 47-frame HEVC Main 10 metadata. The implementation was therefore
rejected again and reverted. Do not retry this isolated direct-padding change
without a materially different end-to-end design or evidence that removes the
inference and queue variance masking its local gain.
