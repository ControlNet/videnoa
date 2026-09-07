# GPU utilization performance contract - 2026-08-28

## Bottom line

Sustained `100%` from `nvidia-smi utilization.gpu` is not a valid production
objective for Videnoa's finite `SR -> FI -> encode` pipeline. It is a sampled,
whole-device kernel-duty signal, not process-attributed throughput, SM occupancy,
Tensor Core efficiency, or useful work. The primary objective must be verified
end-to-end output throughput at fixed output correctness, quality, memory,
energy, and stability.

The current A30 evidence already rejects gauge chasing. The best observed
`SR=1/FI=4` median delivered `7.93` output FPS at only `31.40%` active GPU mean;
`SR=1/FI=3` delivered `7.76` FPS at `30.69%` while saving `2432 MiB` process
VRAM. The two medians differed by `2.12%`, their ranges overlapped, and their
means differed by only about `0.35%`, so `1/3` remains the better measured
throughput/VRAM Pareto point.

## Falsifiable primary objective

For the fixed A30 contract workload (warmed TensorRT, 1080p 120-frame input,
4K 239-frame HEVC output), maximize:

```text
end_to_end_output_fps = 239 / process_wall_seconds
```

The conservative equivalent is to minimize complete invocation wall time.
Cache-hot steady-state pipeline rate is reported separately and never replaces
the user-visible wall metric.

A general production candidate is retained only when five alternating-order,
matched baseline/candidate pairs show:

- at least `10%` median end-to-end wall reduction for an architectural or
  resource-increasing change;
- a bootstrap 95% confidence interval for paired uplift whose lower bound is
  greater than zero and an effect larger than baseline variance;
- unchanged output contract and all resource/stability gates below.

Candidate-specific predeclared gates may be lower for bounded mechanisms:
auxiliary streams require at least `5%` FI-stage improvement plus full-wall gain
beyond variance; CUDA Graph requires at least `5%` FI-stage and `3%` full-wall
improvement; device-resident handoff requires at least `10%` full-wall
improvement. Thresholds must be declared before measurements.

## Secondary metrics

Utilization metrics are explanatory, not voting metrics:

- `nvidia-smi utilization.gpu`: full-wall and active-window mean, p50, p90,
  p95, max, sample count, and share below 50%; record raw 200 ms samples, GPU
  UUID, other processes, clocks, power, temperature, and P-state.
- DCGM: Graphics/SM Active, SM Occupancy, Tensor Active, DRAM Active, and PCIe
  transmit/receive. These distinguish duty, warp residency, compute pipeline,
  memory, and transfer pressure.
- Nsight Systems: process-correlated CUDA API, kernel, memcpy, synchronization,
  stream overlap, host gaps, queue fill, and drain timeline.
- Nsight Compute: achieved occupancy, Speed-of-Light compute/memory throughput,
  Tensor pipeline activity, arithmetic intensity, DRAM throughput, and roofline
  distance for representative kernels.
- Pipeline metrics: per-stage service rate, receive/send wait, queue occupancy,
  first-frame latency, p95 latency, setup, steady state, and drain.
- Resource metrics: process VRAM, device VRAM, RSS, pinned memory, power,
  energy/output-frame, clocks, temperature, and throttling reasons.

## Measured ceilings and bounds

1. The single-lane baseline FI service was `373.8 ms/pair` for 119 pairs,
   totaling `44.4822s`, or `86.22%` of the `51.59s` wall. Perfect width-2 FI
   overlap gave an optimistic `29.35s` wall floor and `1.758x` speedup ceiling.
   The fastest width-2 run reached `29.99s`; the three-run median was `33.21s`
   (`1.553x`). Extra lanes therefore had little remaining width-only headroom.
2. In the independent 4x4 sweep, `SR=1/FI=4` reached `30.141s` and `7.93 FPS`.
   Moving from `1/1` to `1/4` improved output FPS by `61.19%`, but active GPU
   mean rose only from `20.38%` to `31.40%`, while process VRAM increased by
   `7294 MiB` (`215.35%`). Throughput and the gauge do not scale together.
3. In representative `1/4` run 2, parallel FI wall was `22.405s` for 239
   outputs (`10.667 output FPS` effective) and FI postprocess consumed
   `21.531s` (`11.100 output FPS`). Improving FI inference alone has only about
   `4.06%` steady-state headroom before postprocess becomes the limiter; a large
   next gain must move both sides of the FI/device-host seam or another critical
   path.
4. Full-invocation useful GPU duty cannot be 100% for this finite lifecycle.
   The measured `1/4` run had a confirmed CPU-only mux/property-edit tail of
   about `1.311s`; even granting 100% useful GPU duty for every other instant,
   useful process-attributed duty over the `30.141s` wall is below `95.65%`.
   Session setup, queue fill/drain, CPU conversion, synchronization, transfer,
   and encode dependencies lower the practical ceiling further. A whole-device
   gauge can still print 100% during those intervals if unrelated work runs,
   which would be contamination rather than Videnoa progress.

## Counterexamples to gauge optimization

- FI-only workers `2 -> 3`: active GPU mean rose `18.35% -> 18.73%`, while
  throughput fell `20.69 -> 20.52 FPS` (`-0.83%`) and process VRAM rose
  `1897 -> 2539 MiB` (`+33.84%`). This is a measured counterexample.
- FI-only workers `2 -> 4`: sampled GPU max rose `33% -> 37%`, while throughput
  fell `20.69 -> 19.25 FPS` and process VRAM rose to `3187 MiB`.
- A useless spin kernel, duplicate compute, or redundant copy can drive duty
  toward 100% while stealing compute/memory bandwidth and lowering outputs/s.
- A slower tactic or power/thermal-throttled kernel can remain continuously
  active, raising duty while completing less work per second.
- An unrelated process can raise whole-device utilization while contending with
  Videnoa and reducing Videnoa throughput; the current metric is not
  process-attributed.
- Larger batches or worker pools can reduce host gaps but add fill/drain,
  ordering, context contention, memory pressure, and latency; the finite job can
  get slower even when its steady window looks busier.
- Cropping the metric to a hand-picked active window can increase reported
  utilization without changing any output or wall time.

## Retain and reject gates

Evaluate in this order:

1. Identity: same commit base, release mode, input/model/preset hashes,
   runtime/driver, GPU UUID, clocks/power policy, codec settings, warmed state,
   and isolated TensorRT cache. Keep every run and alternate execution order.
2. Correctness: exit zero, no fallback/error, 239 ordered frames, matching
   dimensions/rate/pixel format/timestamps/scene semantics, and identical
   decoded `framemd5` unless a quality tolerance was declared before testing.
3. Throughput: pass the predeclared end-to-end effect and uncertainty gate.
   Local stage improvement without full-wall improvement is rejected.
4. Resources: peak VRAM below 90% of device capacity and at least 2 GiB
   headroom, with 4 GiB preferred for multi-session designs; report RSS and
   pinned memory; energy/output-frame regression over 10% requires separate
   review.
5. Stability: no OOM, leak/growth, rebuild storm, deadlock, ordering failure,
   cancellation hang, thermal throttling, or degradation across ten consecutive
   runs and one long-video soak.
6. Causality: profiler evidence must match the proposed mechanism. A real wall
   gain may be retained as an observed phenomenon if the root-cause story is
   withdrawn; no wall gain means reject regardless of utilization.

Hard decisions:

- Utilization up, throughput flat/down: reject.
- Throughput up, correctness/quality/resource/stability fails: reject.
- Throughput gain within noise or below the predeclared threshold: reject.
- Utilization flat/down, throughput passes all gates: retain.
- Throughput and resources form a Pareto trade-off: prefer the lower-resource
  point unless the speed difference is reproducible and worth the cost.

## Metric semantics sources

- NVIDIA `nvidia-smi`: `utilization.gpu` is the percent of the past sample
  period during which one or more kernels executed. It is device-level duty,
  not occupancy or throughput:
  https://docs.nvidia.com/deploy/nvidia-smi/index.html
- NVIDIA DCGM profiling metrics distinguish SM Active, SM Occupancy, Tensor
  Active, DRAM Active, and PCIe activity:
  https://docs.nvidia.com/datacenter/dcgm/latest/user-guide/feature-overview.html#profiling-metrics
- NVIDIA Nsight Compute defines occupancy as active warps relative to possible
  active warps and explicitly notes that higher occupancy does not always
  produce higher performance:
  https://docs.nvidia.com/nsight-compute/ProfilingGuide/index.html

## Evidence

- `.omo/benchmarks/combined-grid-20260828/results/SUMMARY.md`
- `.omo/benchmarks/combined-grid-20260828/results/combinations.csv`
- `.omo/benchmarks/combined-grid-20260828/results/runs/sr-1-fi-4-run-2/videnoa.log`
- `.omo/benchmarks/arbitrary-workers-20260828/results/SUMMARY.md`
- `.omo/knowledges/arbitrary-inference-workers-2026-08-28.md`
- `.omo/ulw-research/20260827-195139/REPORT.md`
- `.omo/ulw-research/20260827-195139/wave-1-skeptic-roofline.md`
