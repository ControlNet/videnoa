# FP16 terminal RGB parallel conversion benchmark (2026-08-24)

## Workload

- Git baseline: `986f9dcd1d7153fde59fe9880f06ece822df8931`
- Preset: `presets/anime-2x-upscale.json`
- Input: `bench_per_commit_fixture_1080p_120f.mkv`, 1920x1080, 120 frames
- Model: AnimeJaNai V3 L1 Sharp HD x2 FP16
- Backend: warmed TensorRT on NVIDIA A30-24C
- Candidate binary SHA-256: `aad0d23f542b6213a01aa8b63326c5f32fba17af3332c64f571ed6de8c8fc637`

## Runtime decomposition

Temporary per-frame timing over one warmed run established the steady-state terminal FP16 SR cost:

- Input preparation: median `4.70 ms`, total `625.9 ms`
- ORT/TensorRT run and output availability: median `23.80 ms`, total `2961.6 ms`
- FP16 NCHW to RGB8 conversion: median `48.90 ms`, total `5912.1 ms`

The CPU RGB conversion was the largest measured sub-stage and was selected before provider tuning.

## Accepted implementation

`f16_nchw_to_rgb` divides output rows into two independent bands and executes them on a dedicated
two-thread Rayon pool. Each worker retains the existing half-to-f32 bulk conversion, scalar
quantization, row order, crop stride, and output ownership. The separate `f16_bits_nchw_to_rgb`
fallback remains sequential because it is not the measured terminal FP16 SR path.

Unrestricted row parallelism and four bands were rejected because they overfed the encoder, raised
queue occupancy, and increased memory. Two bands balanced the SR stage with NVENC while preserving a
large inference-stage reduction.

## Final interleaved A/B

Order: baseline/candidate, candidate/baseline, baseline/candidate. All runs used frozen binaries,
the same TensorRT cache, `/usr/bin/time -v`, stage summaries, and 200 ms GPU telemetry.

| Metric | Baseline runs | Candidate runs | Median change |
|---|---|---|---|
| Wall time | 16.00, 15.51, 15.71 s | 14.10, 14.34, 13.94 s | 15.71 -> 14.10 s, `10.2%` faster |
| SuperResInference | 10014, 9666, 9584 ms | 7381, 7382, 7535 ms | 9666 -> 7382 ms, `23.6%` faster |
| Peak RSS | 5176640, 5176688, 5176852 KiB | 5297796, 5382696, 5248992 KiB | 5176688 -> 5297796 KiB, `2.3%` higher |
| Peak VRAM | 1653, 1653, 1653 MiB | 1653, 1653, 1653 MiB | unchanged |

The median RSS increase is below the predeclared significant-regression threshold and reflects the
faster producer filling more of the existing four-frame 4K RGB queue. A dedicated two-thread pool
avoids creating a machine-wide Rayon worker pool.

## Correctness

- All six decoded `framemd5` files have SHA-256
  `6b68085e0dde975988bb7e978c4d2ea461d8b2df4ec7ab5ec2a6d33c49bedb2b`.
- Output metadata remains HEVC, 3840x2160, `yuv420p10le`, 120 decoded frames.
- Targeted tests cover truncation, row order, and cropping across padded row/column strides.
- Core suite: `529 passed`, `9 ignored`; crash-hook integration tests: `3 passed`.
- Release workspace build succeeded.
- Workspace-wide rustfmt remains blocked by pre-existing drift in `crates/app/src/lib.rs`.

## Rejected or blocked follow-ups

- Host-only `Vec<u16>` to `Vec<f16>` zero-copy: rejected previously at `-0.25%` wall change.
- Persistent pre-bound output with `ort 2.0.0-rc.11`: blocked by duplicate output ownership and
  pinned-output extraction defects documented in `tensorrt-host-materialization-benchmark-2026-08-23.md`.
- CUDA device output and GPU RGB conversion: highest remaining upside, but requires a CUDA transfer
  wrapper or FFI, stable device buffers, explicit stream synchronization, and broader QA.
