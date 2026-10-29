# videnoa-dev repository retired (2026-10-29)

## What was deleted

`ControlNet/videnoa-dev` was a private repository. It held:

- the pre-release history: 249 commits from 2026-02-09 to 02-22, from
  "init: cargo workspace" up to the performance dashboard;
- early documents that were never imported into `videnoa`: `TECH_REPORT.md`,
  `BENCHMARKS.md`, `.beads/` issues and `.sisyphus/` plans;
- spike examples and bench scripts.

`videnoa` started from its final code in `9d97b0c` ("upload code",
2026-02-22), without that history. The repository had no issues, PRs,
releases, wiki pages or Actions artifacts.

The user reviewed what was unique and deleted the repository on 2026-10-29.
The two documents described the February 2026 pipeline:

- FFmpeg 4.4, TensorRT 10.7, three crates, the `anime-enhancer` binary;
- numbers measured on an A30.

The later `.omo/knowledges` performance notes supersede both, in particular
`combined-sr-fi-bottlenecks-2026-08-24.md`,
`tensorrt-fp16-optimization-report-2026-08-23.md` and
`a40-vs-a30-benchmark-2026-09-10.md`.

## Findings worth keeping (from its BENCHMARKS.md, A30, February 2026)

These are negative results that are not recorded anywhere else. The pipeline
has changed since, so treat them as hints rather than rules.

- **NCHW to RGB conversion inside the interpolation loop:** 35% slower.
  - Moving the conversion out of the encoder serialized it behind RIFE
    inference.
  - While it ran in another thread, it overlapped with the GPU work. The
    later fix was a dedicated RIFE postprocess stage.
- **A CPU mutex serializing SR and RIFE inference:** about 6% slower.
  - With tensor pass-through, SR and RIFE are already serialized by the data
    flow.
  - The RIFE slowdown in combined runs (431 ms to 602 ms per pair) came from
    GPU memory bandwidth contention, which a lock cannot address.
- **Zero-copy `f16`/`u16` reinterpretation between SR micro-stages:** no
  measurable gain. About 2 ms per frame against an 80 ms frame.
- **RealESRGAN 4x FP32 at 1080p, full frame (`tile_size=0`):** runs out of
  memory on 24 GB (its Resize node alone requests 8.5 GB). That model needs
  tiled inference.
