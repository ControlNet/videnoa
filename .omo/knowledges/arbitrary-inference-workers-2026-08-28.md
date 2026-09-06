# Arbitrary FI/SR inference workers — 2026-08-28

## Contract

`FrameInterpolation.num_workers` and `SuperResolution.num_workers` accept every positive integer representable by the workflow `Int` type. Zero is rejected at the node boundary. There is no silent clamp and no fixed application-level upper bound; session allocation and thread-spawn failures must retain lane context in their error chain.

## Ownership and ordering invariants

1. Each lane owns an independent ORT/TensorRT session, execution context, binding lifetime, model buffers, and worker thread.
2. Initialize sessions sequentially so only one lane can build or load a TensorRT cache at a time.
3. Snapshot SR input dimensions before applying scale. Every SR lane must resolve cache identity from those pre-scale dimensions.
4. Concatenated FI parallelizes only inference. ThreeInput FI parallelizes the full node and disables cross-pair NCHW cache because a lane may receive non-consecutive pairs.
5. SR FP16 non-tiled models parallelize inference only. FP32 or tiled SR parallelizes the full node.
6. Bound in-flight work to the lane count and emit only consecutive job/pair IDs from the reorder buffer.
7. Preserve timestamps, scene-change flags, final-frame behavior, cancellation semantics, and full worker error/panic propagation.
8. Reserve sender, handle, and lane vectors with fallible allocation before spawning or constructing workers.

## A30 1080p benchmark decision

The accepted API range is not a performance recommendation:

- FI-only: 2 workers is best (`20.69 fps`, `1.130x`); 3 is flat and 4 regresses.
- SR-only: additional workers do not improve end-to-end throughput; keep default 1.
- SR+FI with both nodes constrained to the same value: 3/3 is the best diagonal point (`7.80 fps`, `8949 MiB` Videnoa VRAM), but this is not the independent optimum.

The independent 4×4 SR/FI sweep supersedes the old diagonal recommendation:

- Lowest observed median: SR=1/FI=4, three-run median `30.141s`, `7.93 fps`, descriptive `1.612x` versus the single current 1/1 run, and `10681 MiB` maximum sampled Videnoa process VRAM. Selective repetition and overlapping ranges mean this is not a statistically established throughput winner.
- Recommended A30 choice: SR=1/FI=3, three-run median `30.780s`, `7.76 fps`, and `8249 MiB` maximum sampled Videnoa process VRAM. Its observed median is `2.12%` slower than 1/4 while saving `2432 MiB`; the means differ by only about `0.35%`.
- Six combinations within about `2.7%` of the first-run winner were measured three times, with the screening run retained in each median; the other ten matrix points remain one-run exploratory measurements.
- Increasing SR workers does not produce a stable combined-workflow gain. Keep SR at 1 and tune FI independently.
- FI=1 is the dominant bottleneck; FI=3/4 reaches the current throughput plateau. FI=2 remains a lower-memory option, but its independent matrix point has only one sample.

Keep general defaults conservative. For this exact A30 24 GiB workload, prefer 1/3 for the measured throughput/VRAM trade-off; use 1/4 only as the lowest-observed-median option when the extra sampled VRAM is acceptable. A true winner requires a fresh predeclared confirmatory series with equal repetitions and screening runs excluded from estimation. Do not infer a universal optimum from whole-device `nvidia-smi` utilization because unrelated processes and non-SM bottlenecks affect that metric.

## Evidence

- `.omo/benchmarks/arbitrary-workers-20260828/results/SUMMARY.md`
- `.omo/benchmarks/arbitrary-workers-20260828/results/summary.csv`
- `.omo/benchmarks/arbitrary-workers-20260828/results/runs/`
- `.omo/benchmarks/combined-grid-20260828/results/SUMMARY.md`
- `.omo/benchmarks/combined-grid-20260828/results/combinations.csv`
- `.omo/benchmarks/combined-grid-20260828/results/runs/`
- `.omo/ulw-research/20260827-195139/REPORT.md`
