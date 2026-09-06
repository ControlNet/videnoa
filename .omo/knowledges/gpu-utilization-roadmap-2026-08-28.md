# GPU utilization and throughput roadmap - 2026-08-28

## Production objective

Maximize verified end-to-end output throughput at fixed correctness, quality,
memory, and stability. Do not optimize for sustained
`nvidia-smi utilization.gpu == 100%`: it is a sampled whole-device kernel-duty
signal, not process-attributed throughput, occupancy, Tensor Core efficiency,
or useful work.

For the warmed A30 contract workload, report `239 / process_wall_seconds` and
retain a candidate only when decoded output, frame order/count, codec contract,
VRAM headroom, and stability remain valid.

## Runtime evidence

The representative `SR=1/FI=3` Nsight Systems trace covered 239 TensorRT
enqueue ranges over `22.843169664s`:

- `cudaStreamSynchronize`: 733 calls, `8.151s` cumulative API time.
- `cudaMemcpyAsync`: 673 calls, `5.821s` cumulative API time.
- Each of the three FI inference threads spent about `2.06-2.26s` in stream
  synchronization and `1.58-1.73s` in memcpy API calls.
- FI preprocessing cost `118-157ms/frame` before the accepted change.
- The output remained HEVC 3840x2160, `yuv420p10le`, 239 frames, with decoded
  hash `39feb41b792f5efa6d2cc2bdcc63405bd62d5bd9959277352669db5c887ee5f2`.

Nsight Systems 2023.4 did not export kernel or GPU-memory activity tables for
this CUDA 12.8 run. Nsight Compute could not collect counters because the user
lacks permission to access NVIDIA performance counters. Occupancy and SOL
claims are therefore intentionally not made.

## Accepted optimization

`FrameInterpolationPreprocess` previously converted each 4K SR tensor into an
unpadded FP32 vector, allocated a second padded array, copied the full tensor,
and transferred the padded allocation downstream. At 3840x2160 this involved
roughly 99.5 MiB of unpadded FP32 storage plus roughly 100.3 MiB of padded FP32
storage per frame.

The accepted implementation converts FP16 values directly into the final
padded FP32 array in bounded 4096-element chunks, fills reflection padding in
place, and transfers that allocation downstream. The legacy `frame_to_nchw`
entry point delegates to the same implementation, removing the former unsafe
u16-to-f16 slice reinterpretation and preventing implementation drift.

Five alternating baseline/candidate pairs produced:

| Metric | Baseline | Candidate | Change |
|---|---:|---:|---:|
| End-to-end wall median | 31.506s | 30.119s | -4.4% |
| FI preprocess median | 152.7ms/frame | 78.7ms/frame | -48.5% |
| FI inference stage wall median | 24.053s | 22.660s | -5.8% |
| Peak process VRAM | 8247-8249 MiB | 8247-8249 MiB | unchanged |

The candidate won four of five pairs. All ten runs produced 239 ordered frames
and the same decoded hash. The full-wall gain is smaller than the local gain
because pipeline stages overlap and downstream postprocess/encode work remains.

## Rejected or deferred options

- More FI workers: existing width sweeps show workers above the current Pareto
  point can raise sampled utilization and VRAM while reducing throughput.
- Gauge chasing: duplicate compute, redundant copies, or unrelated GPU work can
  raise device utilization while reducing useful outputs per second.
- CUDA Graphs: defer until a stable device-resident input/output path exists and
  graph capture compatibility is demonstrated for ORT/TensorRT execution.
- Device-resident SR-to-FI handoff: highest architectural upside, but it requires
  supported device tensors, shared stream/ownership rules, and a redesign of
  frame lifetime and encoder boundaries. Require at least 10% full-wall gain,
  identical decoded output, and at least 2 GiB device headroom.
- GPU-native postprocess/encode: FI postprocess remains about 90-100ms/frame and
  CPU RGB delivery to FFmpeg remains a critical downstream seam. Investigate a
  CUDA/NV12 or P010 path only as a separate correctness-gated project.

## Next experiments

1. Prototype device-resident ORT values across SR and FI using an explicit
   ownership type; measure D2H/H2D elimination before integrating encoding.
2. Measure FI postprocess conversion separately and evaluate a fused device
   crop/quantize/colorspace kernel.
3. Upgrade Nsight Systems or obtain counter permissions before making occupancy,
   Tensor Active, or roofline claims.
4. Preserve `SR=1/FI=3` as the A30 throughput/VRAM recommendation until a wider
   matched experiment proves another point materially faster.

## Accepted FI FP32 postprocess optimization

The single-lane FI FP32 NCHW-to-RGB conversion was a downstream backpressure
seam. Running two complete frame conversions concurrently was rejected because
it removed inference send wait but increased inference and conversion time
through CPU/memory-bandwidth contention.

The retained design keeps one postprocess frame in flight and divides only that
frame into two disjoint row bands on a dedicated two-thread Rayon pool. Five
alternating frozen-baseline/candidate pairs measured a `+6.54%` paired-median
full-wall gain. Independent medians were `30.38s -> 28.12s`; FI postprocess was
`92.1 -> 75.0ms/frame`; inference send wait was `38.7 -> 12.5ms`; process FB
remained `8247-8249 MiB`. All ten A/B outputs had the established decoded hash.

One early candidate pilot aborted during native process teardown after the
workflow had completed and written the correct output. The safe Rust conversion
had no identified indexing or lifetime defect. A subsequent teardown soak of
20 candidate and 5 baseline independent processes, with `MALLOC_CHECK_=3`, had
25/25 zero exits and 25/25 matching hashes. Preserve the isolated abort as a
native-stack residual risk and capture a core/native backtrace if it recurs.

For this workload, prefer bounded within-frame data parallelism over adding
whole-frame residency. The latter increases memory traffic and can reduce useful
TensorRT throughput even when queue backpressure metrics improve.

## v0.1.1 release comparison - 2026-08-31

Five alternating matched pairs compared tag `v0.1.1` (`20e4ef3`) against
current `9fc15d7` on the same A30, dependencies, models, warmed TensorRT cache,
input, encoder, and output hash.

The practical upgrade comparison uses the only topology available in v0.1.1
(single FI worker) versus the current recommended `SR=1/FI=3` topology. Paired
median wall fell by `40.57%`, equivalent to `1.683x` speedup or `68.26%` more
output frames per second. Median FI inference stage wall fell from `46.097s` to
`23.448s`; sampled process FB rose from `3387 MiB` to `8249 MiB`.

A second same-topology comparison ran both binaries with the v0.1.1 preset and
one FI worker. Paired median throughput improved only `1.87%`; FI postprocess
still improved from `90.7` to `70.5ms/frame`. This distinguishes the sources of
gain: independent three-lane FI sessions provide the version-level throughput
jump, while copy and RGB conversion optimizations reduce host-side work and
backpressure but have modest full-wall impact with one inference lane.

Both comparison sets had 10/10 zero exits and 10/10 exact output hashes, with
identical external GPU process snapshots before and after every run. See
`.omo/benchmarks/v0.1.1-comparison-20260831.md` for the full pair table.

## Validation evidence

- `cargo check -p videnoa-core -p videnoa-app --all-targets`
- `cargo test -p videnoa-core -p videnoa-app`
- `cargo build --release -p videnoa-app`
- `rustfmt --check crates/core/src/nodes/frame_interpolation.rs`
- Final CLI QA exercised `--help`, a missing-workflow error, and the warmed
  TensorRT `SR=1/FI=3` workflow. The output was HEVC 3840x2160,
  `yuv420p10le`, `5994/125`, 239 frames, and its `framemd5` stream SHA-256 was
  `39feb41b792f5efa6d2cc2bdcc63405bd62d5bd9959277352669db5c887ee5f2`.
- `.omo/benchmarks/combined-grid-20260828/results/`
- `/tmp/opencode/videnoa-fi-preprocess-ab/` during the validation session

The full workspace release build is independently blocked by the desktop Tauri
build script referencing a stale permissions path under the previous checkout’s
`target/` directory; the affected core and CLI
packages build and test successfully.
