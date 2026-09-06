# Super-resolution performance roadmap (2026-08-24)

## Current fixed workload

- Preset: `presets/anime-2x-upscale.json`
- Input: 1920x1080, 120 frames
- Model: AnimeJaNai V3 L1 Sharp HD x2 FP16
- Backend: warmed TensorRT engine on NVIDIA A30-24C
- Current steady-state CLI throughput: about 12.07 FPS
- Current end-to-end throughput: about 7.34 FPS

## Measured critical path

Candidate stage summaries across three runs:

- Decoder reported effectively zero work and waited about 69-72 ms/frame on downstream capacity.
- SuperResPreprocess used about 16-21 ms/frame and then waited about 59-68 ms/frame.
- SuperResInference used about 83-87 ms/frame with almost no receive or send wait.
- Encoder used about 25-33 ms/frame and waited about 54-58 ms/frame for inference output.

Therefore SuperResInference is the only continuously saturated stage. Decode, preprocess, and
encode already overlap with it and are not the first optimization targets for this workload.

The decoder timing instrumentation is misleading because the timer starts after Iterator::next()
has already produced the frame. This does not change the critical-path conclusion because decoder
send wait is very high, but future decode comparisons need external FFmpeg timing or corrected
instrumentation.

## Source-level costs in current FP16 SR path

`SuperResPreprocess` creates a `Vec<u16>` frame payload from its reusable FP16 ndarray. The inference
stage immediately allocates `Vec<f16>`, reconstructs an ndarray, clones during no-op padding for
aligned 1080p input, and clones again into `Tensor::from_array`.

The FP16 micro-stage uses ordinary `session.run()`, not production IoBinding. The output is extracted
to CPU and read immediately for terminal RGB conversion, forcing a device-to-host boundary and a
completion synchronization for every frame.

## Nsight Systems evidence

Profile artifact: `.omo/benchmarks/videnoa-trt-batch-bench/sr-current.nsys-rep`

The 120-frame run reported:

- `cudaStreamSynchronize`: 364 calls, about 1.641 s cumulative API time
- `cudaMemcpyAsync`: 252 calls, about 1.087 s cumulative API time
- `cudaMalloc`: 27 calls, about 144 ms cumulative API time

The trace did not contain CUDA kernel activity in this environment, so it cannot yet separate model
kernel time from transfer time precisely. It is nevertheless strong evidence that transfer and
synchronization are material and should be attacked before small CPU-loop changes.

## Existing batch experiment

Ignored artifacts under `.omo/benchmarks/videnoa-trt-batch-bench/` already compare batch 1, 2, and 4.
All batch outputs matched exactly. Across the three logs, batch 4 improved total throughput only about
2-4% over batch 1. Output copy remained about 29 ms/frame and dominated the measured benchmark.

Batching is therefore a secondary experiment, not the next implementation target.

## Ranked next experiments

1. Persistent FP16 IoBinding with fixed-shape reusable buffers.
   - Avoid rebuilding a binding and output allocation per frame.
   - Preallocate stable input/output storage for 1080p -> 2160p.
   - Compare ordinary `session.run`, persistent host input + pinned output, and true CUDA device I/O.
   - Measure stage latency, H2D/D2H calls, synchronization calls, FPS, RSS, and VRAM.

2. TensorRT EP CUDA Graph replay after stable addresses exist.
   - Test `trt_cuda_graph_enable` only with fixed shape and persistent buffer addresses.
   - Use the same correctness and three-run benchmark protocol.
   - Reject if it adds cache instability or fails to reduce CPU launch/synchronization overhead.

3. Remove FP16 micro-stage host copies.
   - Replace the `Vec<u16>` -> `Vec<f16>` reconstruction and aligned-padding clone.
   - Prefer an owned FP16 buffer representation or borrowed tensor view that can move across the
     micro-stage without re-encoding each sample.
   - Expected gain is CPU/memory-bandwidth reduction; it may not materially change GPU kernel time.

4. GPU-side terminal conversion and GPU-resident frame transport.
   - Convert FP16 NCHW to encoder-compatible pixels on CUDA rather than extracting full FP16 output
     and converting on CPU.
   - A larger version keeps SR output resident for downstream FI.
   - This has the largest architectural upside but also the highest integration and correctness risk.

5. TensorRT builder tuning and model inspection.
   - Benchmark `trt_auxiliary_streams` values `-1`, `0`, and a small positive value.
   - Compare builder optimization levels 3 and 5 offline with a timing cache.
   - Inspect TensorRT partitioning, layer fusion, fallback nodes, and tactics.
   - Engine/timing caches mainly improve startup; only retain controls that improve warmed latency.

6. Batch 2/4 production pipeline only after transfer optimization.
   - Current isolated gain is small because per-frame output copy dominates.
   - Re-evaluate after persistent device I/O removes the copy bottleneck.

## Rejected experiment: host-side FP16 zero-copy input

Tested a safe `Vec<u16>` to `Vec<f16>` allocation reinterpret plus direct aligned tensor input on
the fixed 1080p, 120-frame workload. Three interleaved warmed baseline/candidate pairs used binaries
built from the same `Cargo.lock` and the same TensorRT cache.

- Median wall time: baseline 15.84 s, candidate 15.88 s (-0.25%).
- Median `SuperResInference`: baseline 9601 ms, candidate 9522 ms (+0.83%).
- Median peak RSS: baseline 5,163,868 KiB, candidate 5,134,608 KiB (+0.57%).
- Peak GPU memory: unchanged at 1653 MiB.
- All six decoded 120-frame outputs had identical `framemd5` content and 3840x2160 HEVC metadata.

The candidate failed the predeclared 2% median end-to-end acceptance threshold and was reverted.
Do not retry this host-only change in isolation; fold it into persistent device I/O work where
transfer and synchronization costs can also be removed.

## Lower-priority items for this workload

- Decoder/NVDEC: upstream is already blocked by inference, and FFmpeg still has to produce CPU RGB.
- Encoder/NVENC: current encoder stage is faster than inference and overlaps with it. It matters for
  product speed/quality tradeoffs but is not the current SR throughput limiter.
- Channel size: high upstream send wait reflects the inference bottleneck. Increasing the buffer will
  increase memory use without improving steady-state throughput unless a trace shows GPU starvation.
- Additional SIMD on RGB loops: useful only after transfer/synchronization and model execution improve.

## Compatibility caution

Official ORT TensorRT compatibility tables do not list ORT 1.23.2 with TensorRT 10.7. The local build
works and must remain the benchmark authority, but new provider options should be validated against
the exact loaded `libonnxruntime.so` and TensorRT libraries rather than assumed from package matrices.
