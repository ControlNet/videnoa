# SuperResolution Pipeline Concurrency Experiments - 2026-08-24

## Decision

- Keep the accepted terminal FP16 `DirectRgb` implementation unchanged.
- Reject a separate FP16 postprocess stage: it regressed median end-to-end wall time by 19.2%.
- Reject the two-worker ordered inference candidate: its single observed improvement was within baseline variance and increased peak RSS by about 7.2%.
- A one-worker output handoff outside the session lock was neutral, confirming the lock-boundary change alone is not useful.
- If CUDA-pinned IoBinding output extraction is revisited, test `ort = 2.0.0-rc.12` with explicit `api-24` first. Keep ONNX Runtime 1.23.2 for that isolated experiment.

## Fixed workload

- Preset: `presets/anime-2x-upscale.json`
- Fixture: `bench_per_commit_fixture_1080p_120f.mkv`
- Model: AnimeJaNai V3 L1 Sharp HD x2 FP16
- Input/output: 1920x1080 RGB to 3840x2160 RGB
- Hardware: NVIDIA A30-24C
- Backend: warmed TensorRT cache
- Runtime: ONNX Runtime 1.23.2, `ort 2.0.0-rc.11`

## DirectRgb versus split postprocess

Three interleaved warmed pairs were run with frozen binaries. All six decoded outputs had the same framemd5 SHA256:

```text
6b68085e0dde975988bb7e978c4d2ea461d8b2df4ec7ab5ec2a6d33c49bedb2b
```

| Pair | DirectRgb wall | Split-stage wall |
|---:|---:|---:|
| 1 | 14.32s | 17.24s |
| 2 | 14.66s | 17.61s |
| 3 | 14.99s | 17.47s |
| Median | 14.66s | 17.47s |

Observed effect:

- Split-stage median regression: 2.81s, or 19.2%.
- Peak VRAM remained 1653 MiB.
- Peak RSS was effectively unchanged near 5.27 GB.
- Split inference averaged approximately 91.0-94.2 ms/frame and the separate conversion approximately 34.2-34.7 ms/frame.
- The owned FP16 materialization cost outweighed cross-frame overlap.

## Session-lock handoff and two workers

The candidate consumed `SessionOutputs` to retain an owned `DynValue`, released the shared session lock, and converted to RGB afterward. A two-worker ordered executor candidate shared the same `Arc<Mutex<Session>>`, so GPU execution remained serialized while CPU conversions could overlap.

Correctness:

- The ordered-worker test forced frame 0 to complete after frame 1 and verified downstream order remained 0 through 5.
- The real 120-frame output matched the baseline framemd5 exactly.

Measurements:

| Candidate | Wall | Peak RSS | Peak VRAM |
|---|---:|---:|---:|
| DirectRgb baseline median | 14.66s | about 5.27 GB | 1653 MiB |
| One worker, conversion outside lock | 14.68s | 5.26 GB | 1653 MiB |
| Two workers, ordered merge | 14.25s | 5.65 GB | 1653 MiB |

Conclusion:

- The one-worker result shows the lock handoff is performance-neutral.
- The two-worker result is only 2.8% below the baseline median and sits inside the baseline range of 14.32-14.99s.
- Peak RSS increased by approximately 385 MB, or 7.2%.
- The candidate required a substantial generic executor change for no defensible throughput win, so it was fully reverted.

## ORT upgrade research

- Latest released wrapper during the experiment: `ort 2.0.0-rc.13`.
- Latest native runtime during the experiment: ONNX Runtime 1.29.0.
- Official `ort` commit `23f9501872c77df4e9530fffe62ca2bd1fcadf6b` fixes CPU-accessibility detection for CUDA-pinned memory and is included in rc.12 and rc.13.
- Native `GetBoundOutputValues` in ONNX Runtime 1.23.2 and 1.29.0 creates distinct `OrtValue` handles sharing ref-counted storage. The apparent dual ownership is intentional and does not require a native runtime upgrade.
- rc.13 is a larger migration: ORT 1.28 target, Rust 1.88, CUDA 13 bundled GPU path, and stricter execution-provider feature matching.

Recommended next dependency experiment:

```toml
ort = { version = "=2.0.0-rc.12", default-features = false, features = ["std", "cuda", "tensorrt", "ndarray", "load-dynamic", "half", "api-24"] }
```

Run that as an isolated IoBinding/CUDA-pinned regression harness against the existing external ONNX Runtime 1.23.2 library before considering any production dependency change.

## Artifacts

Ignored evidence is under `.omo/benchmarks/sr-pipeline-experiments-2026-08-24/`. No experimental source changes were retained.
