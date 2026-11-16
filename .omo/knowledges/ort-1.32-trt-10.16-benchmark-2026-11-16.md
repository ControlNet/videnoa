# ONNX Runtime 1.32 / TensorRT 10.14-10.16 / `ort` rc.15 benchmark (2026-11-16)

Follow-up to `onnxruntime-upgrade-assessment-2026-11-15.md`. Measured on the
dev machine (A40-24Q vGPU, sm86, driver 580.65), only while the shared Videnoa
server had no active job (every run checked the jobs API; no run overlapped a
job). Branch `feat/ort-1.32` (local) holds the `ort` rc.15 migration.

## Configurations

| Name | Binary | ONNX Runtime | TensorRT | CUDA libs |
|------|--------|--------------|----------|-----------|
| A (current) | `ort` rc.12 | 1.23.2 | 10.9.0.34 | 12.8 |
| D | `ort` rc.15 | 1.23.2 | 10.9.0.34 | 12.8 |
| B14 / B16 | `ort` rc.15 | 1.32.0 `gpu_cuda12` | 10.14.1.48 / 10.16.1.11 | 12.9 |
| C14 | `ort` rc.15 | 1.32.0 `gpu_cuda13` | 10.14.1.48 (cu13) | 13.0 |
| AT16 | `ort` rc.12 | 1.23.2 | 10.16.1.11 | 12.8 |
| DT16 | `ort` rc.15 | 1.23.2 | 10.16.1.11 | 12.8 |

Each config had its own library directory as the only GPU entry in
`LD_LIBRARY_PATH` and its own TensorRT cache. The mapped libraries were checked
from `/proc/<pid>/maps` for every run. Libraries came from PyPI /
pypi.nvidia.com wheels (`tensorrt-cu12-libs`, `nvidia-*-cu12`) and the ORT
GitHub release archives.

Workloads: the `interpolation-2x-anime-2x` preset (RIFE 2x then AnimeJaNai 2x
SR) and the `interpolation-2x` preset (RIFE only), encoded with x265 or NVENC,
on a 1080p 480-frame FFV1 anime clip. Lossless (FFV1) runs on the 120-frame
fixture were used to compare output. Steady fps is the progress-bar rate; wall
time includes startup.

## Results

FI -> SR preset (3 rounds each, rotating order):

| Config | NVENC wall | x265 wall | Steady fps | Peak RSS (NVENC / x265) | GPU memory |
|--------|-----------|-----------|------------|-------------------------|------------|
| A | 67.4 s | 69.7 s | 7.9 / 7.6-7.8 | 5.08 GB / 5.08 GB | 2267 MiB |
| D | same as A | same as A | +0.9% | same as A | 2267 MiB |
| B14 / B16 / C14 | ~67-72 s | ~67-72 s | -8% / -4% / -7% (NVENC) | 2.0 GB / 3.8 GB | 1807-1915 MiB |
| AT16 | 64.2 s | 65.2 s | 7.8-7.9 / 7.7-7.8 | 2.1 GB / 4.0 GB | 2107 MiB |
| DT16 | 64.6 s | 65.3 s | 7.7-7.9 / 7.7-7.8 | 2.1 GB / 4.0 GB | 2107 MiB |

RIFE-only, NVENC (inference-bound):

| Config | Steady fps | Wall | Peak RSS | GPU memory |
|--------|-----------|------|----------|------------|
| A | 55-60 | 14.6-15.2 s | 5.08 GB | 1917 MiB |
| B14 / B16 / C14 | ~41 (-30%) | shorter (startup) | 1.6 GB | 1457 MiB |
| AT16 | 61.5-67.2 | 9.9-10.4 s | 1.66-1.69 GB | 1757 MiB |

RIFE-only fps is noisy run to run (A ranged 55-73 fps over seven runs, AT16
53-67), so AT16 is "no regression", not measurably faster. The ~30% drop of
the 1.32 configs is well outside that range.

- The RIFE regression comes from the ONNX Runtime 1.32 package (its TensorRT
  EP together with the CUDA 12.9 / 13.0 libraries it was run with), not from
  TensorRT 10.14/10.16: ORT 1.23.2 with TensorRT 10.16 is as fast as A. The verbose log under 1.32 shows the whole RIFE graph on one
  TensorRT node with no memcpy, so the cause is inside the EP's run path; not
  investigated further.
- TensorRT session init with a warm cache: ~4.2 s (TensorRT 10.9) -> 0.6-0.85 s
  (10.14+). Most of the wall-time gain is this startup.
- Host RSS drops in two parts (both independent of the ONNX Runtime version):
  - Anonymous memory: ~4.0 GB (TensorRT 10.9) -> ~1.3 GB (10.16) on the
    FI -> SR preset, 2.75 -> 1.0 GB RIFE-only, warm cache.
  - File pages: TensorRT 10.9 ships one 1.97 GB
    `libnvinfer_builder_resource.so.10.9.0`. TensorRT 10.16 splits it per
    GPU architecture (`libnvinfer_builder_resource_sm75/80/86/89/90/100/120`
    and `_ptx`). When the runtime preloader dlopens every TensorRT file in
    `<exe_dir>/lib` or `/usr/local/lib` (release archive and Docker; it is
    skipped when `ORT_DYLIB_PATH` is set), the 10.9 resource becomes resident
    (1.93 GB file RSS) while the eight 10.16 files are mapped but only touched
    as needed (0.4 GB file RSS for everything).
  - Release-like layout with the preloader active: peak RSS 5.95-6.3 GB ->
    1.65-2.1 GB. The numbers in the tables above were measured with
    `ORT_DYLIB_PATH`/`LD_LIBRARY_PATH` instead (no preload).
- `ort` rc.12 -> rc.15 alone changes neither speed nor memory (D vs A, DT16 vs
  AT16).
- Output: D is bit-identical to A. AT16, DT16, B14, B16 and C14 are
  bit-identical to each other (FFV1 frame md5). Against A (TensorRT 10.9
  engines) PSNR is 71.4 dB average, 66.1 dB minimum: the difference comes from
  the TensorRT version only.
- Cold engine build (RIFE + SR): ~234-240 s for every config, both engines.
  Cold peak RSS 5.3 GB -> 2.5-2.9 GB.
- One cold run of AT16 (SR engine built cold) exited with SIGSEGV (rc=139)
  near the end of the clip, leaving no output. AT16/DT16: 1 crash in 9 runs
  that built an engine, 0 in ~25 warm runs. Seven repeats of the same and full
  cold builds did not reproduce it. A only had 2 cold-build runs, so "never
  seen on 10.9" rests on a small sample. Unexplained; watch for it.
- ORT 1.32 tuning that did not help: TensorRT builder optimization level 5
  (14:40 cold build, still ~40 fps RIFE-only); TensorRT EP CUDA graph fails
  with "operation not permitted when stream is capturing" (CUDA error 900/901).

## Compatibility facts

- ORT 1.24.1+ release packages are built with TensorRT 10.14.1.48; 1.32
  `gpu_cuda12` is built with CUDA 12.9 and still ships both the TensorRT and
  CUDA provider libraries. Its CUDA EP no longer needs cuDNN/cuFFT but loads
  libcurand.
- ORT 1.23.2 (built with TensorRT 10.9) ran with TensorRT 10.16 libraries and
  the CUDA 12.8 runtime without problems apart from the one crash above. This
  is an empirical result, not a documented guarantee.
- TensorRT 11.x cannot be used: ORT links `libnvinfer.so.10`. The newest 10.x
  is 10.16.1.11.
- The TensorRT cache directory name contains the TensorRT and ORT versions, so
  engines rebuild automatically after either changes.
- Library sizes: TensorRT 10.16 builder resources total ~2.9 GB for all
  architectures (10.9: 1.97 GB). The ORT 1.32 `gpu_cuda12` set with CUDA 12.9
  libs was 5.9 GB, the `gpu_cuda13` set 3.9 GB.
- Packaging changes for TensorRT 10.16: `Dockerfile` copies
  `libnvinfer_builder_resource.so.*`, which no longer matches the per-arch
  files; `scripts/tests/docker_slimming_contract_test.sh` checks
  `libnvinfer_builder_resource.so.10.9.0`. Shipping every architecture adds
  ~0.9 GB; the 10.16 resources start at sm75. The Windows DLL set was not
  tested.

## Conclusion

Do not move to ONNX Runtime 1.32 for now: RIFE throughput drops ~30% when
inference-bound and the FI -> SR preset is 4-8% slower; its extra GPU-memory
saving (~190 MiB) does not compensate. The useful upgrade is TensorRT
10.9 -> 10.16 with ONNX Runtime 1.23.2: same steady throughput, 4-7% shorter
wall time from faster startup, ~3 GB less host RSS, 160 MiB less GPU memory,
no RIFE-only regression. ONNX Runtime 1.23.2 with TensorRT 10.16 is not a
combination Microsoft or NVIDIA ships (1.23.2 was built with 10.9), and the
one unexplained cold-build crash happened with it. `ort` rc.15 is a safe
code-quality upgrade with no measured effect. Recheck the ORT 1.3x TensorRT EP regression on later
releases.
