# Two-lane RIFE concurrency on A30 — 2026-08-27

## Decision

Retain exactly two independent TensorRT RIFE sessions for shipped concatenated frame-interpolation workflows. Keep `num_workers` defaulted to `1` for custom and legacy workflows; shipped TensorRT FI presets may opt into `2`.

`inference_concurrency` is a removed name, not a compatibility alias. Reject it with a migration error so stale workflows cannot silently fall back to one worker.

## Why this won

The representative SR→FI workload was not starved by upstream stages. Baseline FI inference consumed `373.8 ms/pair` with only `4.3 ms/pair` receive wait, while active whole-device GPU utilization averaged `19.24%`. A second independently owned batch-1 session exposed cross-inference work without changing the batch-unsafe RIFE graph.

For 1080p 120-frame input → 4K 239-frame HEVC output:

- Single-lane wall: `51.59s`.
- Two-lane walls: `33.21s`, `35.25s`, `29.99s`.
- Two-lane median: `33.21s`, `35.63%` lower wall, `1.553x` throughput.
- Active GPU mean: `19.24%` → `28.13%` across optimized runs.
- Peak observed GPU memory: `4085 MiB` → `6517 MiB`.
- Baseline and all optimized decoded framemd5 hashes: `39feb41b792f5efa6d2cc2bdcc63405bd62d5bd9959277352669db5c887ee5f2`.

FI-only warmed matched control also favored width 2: `11.55s` versus single-lane `12.88s`, with identical decoded hash `a739a306cb0ab27d2b65152b87cbbda07395eae3a88f1b9760df3d971bfbb008`.

## Required implementation invariants

1. Construct sessions sequentially so only one session can build or update the TensorRT engine/profile cache at a time.
2. Each lane owns its Session, execution context, concat buffer, IoBinding lifetime, and output storage.
3. Bound in-flight pairs to the number of lanes; do not retain an unbounded queue of 4K outputs.
4. Share adjacent source boundary frames immutably with `Arc`; do not clone the full payload for each pair.
5. Assign monotonically increasing pair IDs and commit only consecutive results from a `BTreeMap` reorder buffer.
6. Emit the final source frame exactly once after every in-flight pair completes.
7. On worker error or panic, stop new dispatch, propagate the full error, join both workers, and do not emit a partial final frame.
8. On cancellation, stop dispatch and omit the final frame. TensorRT Run remains cooperative rather than preemptive.
9. Reject width 2 for unverified model formats; the implementation only accepts concatenated RIFE.
10. Join every worker handle even after the first uncaught thread panic; preserving only the first error must not short-circuit cleanup.
11. Derive output indices and `output_frames` from successful sends, including the final frame. Measure average send wait over all send attempts so failed output-closure attempts do not skew the denominator.

Post-refactor verification retained the same observable surface: the release FI-only run completed in `11.72s`, emitted 239 frames, and reproduced decoded hash `a739a306cb0ab27d2b65152b87cbbda07395eae3a88f1b9760df3d971bfbb008`. Eleven focused concurrency/metrics tests, the full workspace suite, release build, and independent Oracle review passed.

## Rejected shortcuts

- Do not set RIFE batch to 2. The current ONNX graph contains internal hardcoded batch/channel Reshape constants despite dynamic input metadata.
- Do not fan out a shared `Arc<Mutex<Session>>`; application and ORT TensorRT synchronization serialize it.
- Do not increase channel capacity as a substitute for a second execution context.
- Do not combine the first concurrency experiment with auxiliary streams, CUDA Graph, or IoBinding lifetime changes; attribution would be lost.
- Do not treat `nvidia-smi utilization.gpu` as achieved SM occupancy or as the production success criterion.

## Benchmark protocol

Use warmed, cache-identical runs and keep raw logs, 200 ms GPU CSV, GNU time, FFprobe JSON, decoded framemd5, process list, and engine-cache manifest. A cold TensorRT engine build must be reported separately from warmed wall time.

Primary artifacts:

- `.omo/ulw-research/20260827-195139/REPORT.md`
- `.omo/ulw-research/20260827-195139/data/two-lane-sr-fi-aggregate.json`
- `.omo/ulw-research/20260827-195139/data/two-lane-sr-fi-comparison.csv`
- `.omo/ulw-research/20260827-195139/wave-1-batch-concurrency.md`
- `.omo/ulw-research/20260827-195139/wave-1-transfer-binding.md`

## Next optimization order

1. Profile and remove the SR D2H → host conversion/materialization → FI H2D seam with a device-resident handoff.
2. Run isolated-cache TensorRT auxiliary-stream `-1/0/1/2` A/B experiments.
3. Consider CUDA Graph only after Nsight proves material enqueue/launch gaps and stable binding addresses are implemented.
4. Add long-video soak, DCGM power/energy, and Nsight overlap evidence before considering width 3.
