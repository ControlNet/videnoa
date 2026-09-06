# FP16 terminal output fusion benchmark (2026-08-24)

## Decision

Retain the terminal `FP16 tile_size=0 SuperResolution -> VideoOutput` fusion.

The candidate converts the borrowed ONNX Runtime FP16 output view directly to owned RGB while
`SessionOutputs` is alive. It preserves SuperResolution truncation semantics, leaves the
`SuperResolution -> FrameInterpolation` tensor path unchanged, and uses a shared chunked converter
for the `VideoOutput` FP16 fallback without its previous full-frame FP32 allocation.

## Fixed workload

- Git baseline: `5c46d9527a625f1db5a630ad06eb57f966d61caf`
- Preset: `presets/anime-2x-upscale.json`
- Fixture: `bench_per_commit_fixture_1080p_120f.mkv`
  - SHA256: `58bfbe8722bd801057aa907ae304bebcc6c2b4bafe979b1770c5e52207210fde`
- Model: `models/the_database_AnimeJaNaiV3L1_sharp_HD_x2_fp16_op17.onnx`
  - SHA256: `7666ddb0b9078e9fd47bf961e718a67715ad28894477be588d1fe69a9418a0a5`
- Backend: TensorRT with warmed `trt_cache/`
- Input: 1920x1080, 120 frames
- Output: libx265 CRF 18, `yuv420p10le`, 3840x2160
- GPU: NVIDIA A30-24C
- Frozen baseline binary SHA256:
  `331ced9f8fdd1501717b0944310f387775bf10b6dc0237dbf5772534c0393cb8`
- Candidate binary SHA256:
  `bc8e7e6d6ccc424390fd60ce27413484fae268b814ee63de254f25db20045e9a`

## Results

| Metric | Baseline mean | Candidate mean | Relative change |
|---|---:|---:|---:|
| Steady-state CLI FPS | 9.867 | 12.067 | +22.30% |
| End-to-end FPS | 6.431 | 7.335 | +14.06% |
| Wall time | 18.663 s | 16.363 s | -12.32% |
| SuperRes inference stage | 102.933 ms/frame | 84.433 ms/frame | -17.97% |
| SuperRes postprocess stage | 56.633 ms/frame | 0.000 ms/frame | -100.00% |
| Peak RSS | 5120.3 MiB | 5053.2 MiB | -67.1 MiB |
| Mean GPU utilization | 10.835% | 12.044% | +1.209 percentage points |
| Peak VRAM | 1653 MiB | 1653 MiB | unchanged |

Per-run steady-state FPS:

- Baseline: `9.7`, `10.2`, `9.7`
- Candidate: `12.2`, `11.7`, `12.3`

Per-run wall time:

- Baseline: `18.90`, `18.28`, `18.81` seconds
- Candidate: `15.99`, `16.60`, `16.50` seconds

The improvement exceeds the prior approximately 1% attribution threshold in every run.

## Correctness

All six outputs were:

- HEVC
- 3840x2160
- `yuv420p10le`
- `2997/125` FPS
- 120 decoded frames
- 5.005 seconds
- 1,396,193 bytes

Decoded `framemd5` files for all three baseline and all three candidate outputs had the same SHA256:

`6b68085e0dde975988bb7e978c4d2ea461d8b2df4ec7ab5ec2a6d33c49bedb2b`

Therefore every decoded output frame is byte-identical between baseline and candidate.

## Validation

- `cargo test --workspace --no-fail-fast`: 527 passed, 9 ignored.
- Targeted compile-mode, crop, quantization, and fallback tests passed.
- `cargo build --release --bin videnoa`: passed.
- LSP diagnostics: clean for every changed Rust file.
- Oracle review: no release-blocking findings.

Workspace-wide `cargo clippy --workspace --all-targets --all-features -- -D warnings` is not a clean
repository gate at this baseline. It reports numerous pre-existing warnings, and desktop workspace
compilation also encountered a stale generated Tauri permission path under `target/` referencing
the previous checkout. The only warning introduced by this change was fixed before
the final build.

## Raw evidence

Ignored local artifacts are in `.omo/benchmarks/fp16-fusion-2026-08-24/`:

- Frozen baseline binary
- Three baseline and three candidate logs
- GNU time reports
- GPU CSV samples
- FFprobe JSON
- Encoded outputs
- Decoded `framemd5` files
