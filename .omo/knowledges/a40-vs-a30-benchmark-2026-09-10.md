# A40 versus A30 TensorRT benchmark — 2026-09-10

## Result

On the fixed 1080p/120-frame contract workload, the NVIDIA A40-24Q provides a small gain for isolated nodes and a material gain for the concurrent SR+FI workflow:

| Scenario | Workers | A30 reported wall / FPS | A40 median wall / FPS | Reported throughput change | Log-span normalized change |
|---|---:|---:|---:|---:|---:|
| SR only | SR=1 | 14.462 s / 8.30 | 13.980 s / 8.58 | +3.45% | +1.38% |
| FI only | FI=2 | 11.553 s / 20.69 | 10.680 s / 22.38 | +8.17% | +6.22% |
| SR then FI | SR=1, FI=3 | 30.780 s / 7.76 | 25.910 s / 9.22 | +18.80% | +17.72% |

The practical interpretation is about **1–3% faster for isolated SR, 6–8% faster for isolated FI, and 18–19% faster for the recommended combined workload**. The combined result has three samples on both GPUs and is the strongest comparison. The isolated A30 references each have one sample, so their smaller differences should be treated as directional.

## Measurement contract

- Source commit: `9fc15d7bc2d91969ba347b797ff94fe30d450352` (`Parallelize frame interpolation RGB conversion`).
- A40: `NVIDIA A40-24Q`, 24,576 MiB, compute capability 8.6, driver 580.65.06.
- A30 reference: `NVIDIA A30-24C`, 24,576 MiB, compute capability 8.0, driver 580.65.06.
- Backend: warmed TensorRT FP16 with device/model/shape-specific caches. A40 `sm86` engines were built in a new isolated cache and no A30 `sm80` engine was reused.
- Input: FFV1, 1920×1080, `yuv420p`, 24000/1001 fps, 120 frames, 5.005 s, 56,806,131 bytes.
- Models: AnimeJaNai V3 L1 Sharp HD x2 (`7666ddb0...`) and RIFE v4.26 (`af25762d...`).
- Output: HEVC `yuv420p10le`; SR emits 120 frames at 3840×2160, while FI and combined emit 239 frames at 5994/125 fps.
- A40 repetitions: three cache-hot samples per scenario. GPU and device memory were sampled every 200 ms. Every reported run exited zero and logged successful completion.

The recreated fixture has container SHA-256 `8a88066d...`, while the historical file had `58bfbe87...`. It was rebuilt from the same unchanged `1.mkv` source with the documented command and has the exact historical stream properties and byte length. Matroska mux metadata explains the container hash change. The exact A30/A40 SR decoded-output match provides an end-to-end content check.

The A30 scripts measured wall time around a 200 ms telemetry loop; the A40 scripts used `/usr/bin/time` directly around the process. The `reported throughput change` column compares the published wall results. The normalized column compares the duration between the first structured log timestamp and `Workflow completed successfully` on both GPUs, reducing that harness bias. Normalized medians were 13.715→13.527 s for SR, 10.830→10.196 s for FI, and 29.982→25.469 s for combined.

## A40 samples and telemetry

| Scenario | Cache-hot wall samples | Median active GPU | GPU max | Estimated incremental VRAM |
|---|---|---:|---:|---:|
| SR=1 | 14.09, 13.55, 13.98 s | 10.98% | 20% | 1,027 MiB |
| FI=2 | 10.44, 11.10, 10.68 s | 20.05% | 39% | 1,918 MiB |
| SR=1/FI=3 | 26.04, 25.91, 25.30 s | 32.36% | 48% | 8,031 MiB |

The A40 had one pre-existing Python CUDA process using 733 MiB, and whole-device idle memory was 734 MiB. Incremental VRAM subtracts that stable baseline from peak whole-device memory; it is an estimate rather than process-attributed sampling. Historical A30 process peaks were 959 MiB, 1,897 MiB, and 8,249 MiB respectively, so memory demand remains in the same range.

Whole-device utilization is not a reliable cross-GPU efficiency measure. It shows that the combined workflow exposes more useful parallelism on the A40, but it does not identify whether the gain comes from CUDA cores, Tensor Cores, memory behavior, clocks, or vGPU scheduling.

## Cold cache behavior

Cold TensorRT engine builds are excluded from throughput comparison:

- First combined run, building the A40 SR 1080p and RIFE 4K engines: 10:05.35.
- First FI-only run, building the separate RIFE 1080p engine: 2:45.10.

Subsequent runs loaded the caches in seconds. Real deployments should preserve `trt_cache`; otherwise engine compilation dominates short jobs.

## Output validation

- SR is bit-identical between A30 and A40: decoded framemd5 SHA-256 `6b68085e...`.
- FI is stable across all three A40 hot-cache runs but differs from A30 at the decoded-frame level. Against the A30 output, PSNR is 56.92 dB and SSIM is 0.998284.
- Combined is stable across all three A40 hot-cache runs but differs from A30. Against the A30 output, PSNR is 59.37 dB and SSIM is 0.999054.

The FI-derived differences are very small but prevent a bit-identical claim. They can arise from TensorRT FP16 tactic and floating-point differences between `sm80` and `sm86`, or from the dependency-lock drift described below; this evidence does not isolate the cause. The excluded FI engine-build run also had a different hash from the three cache-hot runs, so future correctness-sensitive benchmarking should keep a fixed serialized engine and compare with a declared PSNR/SSIM tolerance.

## Reproducibility limitation

The source commit, models, ORT library, environment, and workflow settings were fixed, but the historical generated `Cargo.lock` is no longer available. The A30 campaign recorded lock SHA-256 `102ec183...`; rebuilding the historical commit on 2026-09-10 resolved lock SHA-256 `a6f7069a...`. The frozen A40 binary SHA-256 is `199e2ffd...`. Dependency resolution drift is therefore a remaining confounder, especially for the isolated-node differences. The 18–19% combined gain is much larger than the observed run-to-run spread, but exact GPU-only attribution would require rebuilding both GPUs from the same lockfile.

## Evidence

- A40 raw evidence: `.omo/benchmarks/a40-vs-a30-20260910/`
- A30 isolated references: `.omo/benchmarks/arbitrary-workers-20260828/results/`
- A30 combined references: `.omo/benchmarks/combined-grid-20260828/results/`
- A30 decision record: `.omo/knowledges/arbitrary-inference-workers-2026-08-28.md`

The ignored A40 evidence directory retains the frozen binary, resolved lockfile, generated configs, TensorRT engines, outputs, logs, `/usr/bin/time` reports, FFprobe JSON, decoded hashes, and 200 ms GPU CSV files.
