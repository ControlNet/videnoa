# Wan2GP DLSS source review (2026-09-08)

Inspected Wan2GP `362c3467a70e1136ceb52eec95907205a8f88543` and its author's
worker fork `93ab61a6bb92194eaa0458fc9c9434a4a4868808`. Read-only inspection:
no workers, installers, or model inference were executed.

## Features

- Spatial postprocessing: image/batch/video Neural Rendering at 1x, 1.5x,
  1.724x, 2x, and 3x; intensity 0-2; full/half/quarter-resolution depth estimation.
  1x is native-resolution refinement. This is an appearance enhancement path,
  not just resolution conversion, and is not RTX Video SDK integration.
- Temporal postprocessing: integer 2x-6x DLSS FG, gated by worker version,
  runtime capability, and GPU family. Application policy allows RTX 40+ for FG,
  caps non-50-series at 4x, and restricts 5x/6x to RTX 50. NR policy requires
  RTX 30+. These are application checks, not universal SDK requirements.
- Integrated into postprocessing and Media Flow for existing media as well as
  generated outputs. DLSS is optional and installed separately.

## Platform evidence

`runtime.py::unavailable_reason()` explicitly returns `Windows 11 required`
when `os.name != "nt"`, after checking missing files. `dlssg_capabilities()`
returns an empty result on non-Windows. Worker paths end in `.exe`/`.dll` and
are launched directly using subprocess; no Wine launcher or Linux NGX path is
implemented in this integration. Do not confuse overall Wan2GP Linux support
with availability of these optional DLSS processors.

## What the implementation does

Python converts `[3|4, frames, height, width]` tensors into CPU RGBA8 and streams
them through persistent binary stdin/stdout to D3D12 workers. Both processors
use OpenCV DIS by default or optional RAFT (20 iterations), with flow computed
at about 640-pixel width and resized back to the processing dimensions.

NR enables depth guidance by default. It uses Depth Anything V2 (default vitl)
or the configured DA3 metric-large variant. Depth is normalized with 2nd/98th
percentiles and temporally smoothed bounds; it remains estimated rather than
renderer depth. Color, FP16 motion, and FP32 depth enter the depth-aware worker.
That worker calls public DLSS/DLAA feature 1, intercepted by ReShade/RenoDX to
execute NR feature 18. It still depends on the external carrier/runtime stack.

FG does NOT receive Depth Anything output. Its public C++ worker creates a
uniform depth texture filled with `0.5f` (`dlssg_worker.cpp`, line 232). The
Python FG protocol supplies only color, motion, timestamps, and resets. This
constant depth is an upstream implementation approximation, not a proposed
videnoa implementation or a locally introduced test fixture.

FG uses native multi-frame counts rather than the other ComfyUI project's
cascaded 2x grid. It records each generated-frame index in one command list.
Python allocates `(N-1)*scale+1` output frames, copies source frames at integer
positions, and repeats the previous source frame for a cut or disabled
generation. Chunk continuation carries the preceding chunk's last source frame.

The worker protocol streams frames, but the Python wrapper preallocates the
whole output batch in CPU RGBA8. Videnoa should reuse protocol/NGX design ideas,
not copy this batch-memory strategy for long videos. Neither guide inference
nor CPU/GPU transfers are free; README expectations of higher speed than RIFE
are not a reproduced benchmark.

## Source availability and dependencies

Wan2GP's guide refers to `native/dlss5/build.ps1`, but that directory is absent
from the inspected Wan2GP tree. The actual worker sources are available in the
author's separate `DeepBeepMeep/dlss5-visual-enhancer` fork under
`native/WanGP-Adapter/`, including `dlssg_worker.cpp`, `nr_depth_worker.cpp`, and
`build.ps1`. This is a more concrete FG porting reference than a binary-only
worker. Linux still requires a Vulkan/NGX port of D3D12 resource/sync code and
changes to platform detection/launching.

The documented NR setup uses a community-modified, unsigned
`nvngx_dlssnr.dll` 310.8.SF-v2, explicitly identified as outside the public
NVIDIA DLSS SDK. Standard SR/FG runtimes are separately sourced from NVIDIA.
Do not treat the NR setup as equivalent to using the official RTX Video SDK
or automatically suitable for redistribution. No third-party binary was
downloaded or executed for this assessment.

## Sources

- [Wan2GP runtime](https://github.com/deepbeepmeep/Wan2GP/blob/362c3467a70e1136ceb52eec95907205a8f88543/postprocessing/dlss5/runtime.py)
- [Spatial processor](https://github.com/deepbeepmeep/Wan2GP/blob/362c3467a70e1136ceb52eec95907205a8f88543/postprocessing/dlss5/spatial_upsampler.py)
- [Temporal processor](https://github.com/deepbeepmeep/Wan2GP/blob/362c3467a70e1136ceb52eec95907205a8f88543/postprocessing/dlss5/temporal_upsampler.py)
- [Install guide](https://github.com/deepbeepmeep/Wan2GP/blob/362c3467a70e1136ceb52eec95907205a8f88543/docs/DLSS5.md)
- [Worker sources](https://github.com/DeepBeepMeep/dlss5-visual-enhancer/tree/93ab61a6bb92194eaa0458fc9c9434a4a4868808/native/WanGP-Adapter)

Documentation verification: `git diff --cached --check` and
`python /home/zhixi/.codex/skills/secret-guard/scripts/scan_secrets.py staged`.
Expected: no whitespace errors or detected secrets. No application tests or
builds are needed for this documentation-only change.
