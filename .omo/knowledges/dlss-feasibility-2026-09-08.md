# DLSS video feasibility (2026-09-08)

## Evidence and scope

Source review only; no NVIDIA binaries were executed and no GPU performance or
image-quality results were reproduced locally. Inspected ComfyUI repository
revision `c755e274a405a7a47667bd567d489b6845066bcf`.

- [Repository](https://github.com/Konohamaru04/ComfyUI-NVIDIA-DLSS-Frame-Interpolation/tree/c755e274a405a7a47667bd567d489b6845066bcf)
- [Worker protocol](https://github.com/Konohamaru04/ComfyUI-NVIDIA-DLSS-Frame-Interpolation/blob/c755e274a405a7a47667bd567d489b6845066bcf/dlss_engine/frame_interpolation/native.py)
- [Motion estimation](https://github.com/Konohamaru04/ComfyUI-NVIDIA-DLSS-Frame-Interpolation/blob/c755e274a405a7a47667bd567d489b6845066bcf/dlss_engine/frame_interpolation/guides.py)
- [Scheduling](https://github.com/Konohamaru04/ComfyUI-NVIDIA-DLSS-Frame-Interpolation/blob/c755e274a405a7a47667bd567d489b6845066bcf/dlss_engine/frame_interpolation/scheduler.py)
- [Linux setup and limitations](https://github.com/Konohamaru04/ComfyUI-NVIDIA-DLSS-Frame-Interpolation/blob/c755e274a405a7a47667bd567d489b6845066bcf/docs/linux.md)
- [Upstream application](https://github.com/Merserk/dlss5-visual-enhancer)
- [Official DLSS-G integration](https://github.com/NVIDIA-RTX/Streamline/blob/main/docs/ProgrammingGuideDLSS_G.md)
- [Official RTX Video SDK requirements](https://developer.nvidia.com/rtx-video-sdk/getting-started)

## Findings

This is a native runtime integration, not an ONNX model. Python runs persistent
Windows worker processes using binary stdin/stdout. FI uses D3D12 NGX DLSSG;
upscaling uses a ReShade/RenoDX carrier with DLSS SR and Neural Rendering (NR).
The inspected tree ships workers but no C/C++ source for them. The upstream
application's current main tree also had no C/C++ worker source. The Linux
ReShade patch and its build workflow are available; that does not make the
application workers reproducibly buildable from this tree.

FI passes RGBA8, FP16 two-channel motion, rational timestamps, and reset flags.
OpenCV DIS estimates backward optical flow at approximately 640-pixel width and
resizes it. The public protocol supplies no real depth buffer; worker-side
depth handling cannot be established from these Python sources. NVIDIA's
official renderer integration requires depth, motion, and HUD-less color.
Video-derived guides are therefore an approximation of renderer inputs.

Native interpolation requires an exact supported integer multiplier, discovered
from the runtime. Otherwise the scheduler cascades 2x stages, commonly to an 8x
grid, and selects nearest frames for the target timeline. Exact output FPS does
not imply synthesis at every exact target instant. Cascading also multiplies
work and can accumulate interpolation artifacts.

The documented Linux route uses Wine, DXVK, VKD3D-Proton, DXVK-NVAPI, patched
ReShade, native shader compilation, and heap settings. The authors report
testing RTX 5090 / driver 610.57.04 / GE-Proton 11-6. This is experimental
compatibility, not evidence of a native Linux worker or universal GPU support.

Distinguish DLSS SR from direct NR upscaling. In the documented Linux tests,
DLSS SR precedes NR at output resolution; `feature_18_confirmed` is true,
`nr_native_fallback` is true, and `nr_upscaling_active` is false. This does not
prove SR was absent; it means direct low-resolution NR upscaling was inactive.
Do not label feature-18 execution alone as successful direct NR upscaling.

The README's RTX 4060 Ti example reports about 5 seconds of input taking
12.670 seconds for 3x upscale and 153.747 seconds for interpolation to 120 FPS,
166.417 seconds combined. These include media processing and guide estimation,
and are not directly comparable with videnoa's different benchmarks or with
game-engine DLSS timings.

## Fit with videnoa

- `crates/core/src/nodes/backend.rs` builds ORT sessions; DLSS cannot be added
  merely as another execution provider or model-registry entry.
- `FrameProcessor` in `node.rs` and `FrameInterpolator` in
  `streaming_executor.rs` offer useful integration boundaries. Add dedicated
  DLSS stages with persistent worker ownership and explicit capability probing.
- `types.rs::Frame` currently owns CPU RGB/tensor data, so a byte-pipe prototype
  is feasible. It still incurs conversion, IPC, upload, and readback costs.
- DLSS temporal history requires ordered frame delivery and explicit resets.
  Existing parallel processor/interpolator paths must not distribute frames
  across independent sessions without a history-aware design.
- Start with SDR RGB and 2x FI. The worker uses RGBA8; a 10-bit encoder option
  alone does not establish an end-to-end high-precision HDR processing path.
- Keep FFmpeg decode/encode in videnoa when integrating frame-level workers,
  avoiding ComfyUI's extra encoded intermediates.

## Planning estimates (engineering judgment, not measured delivery promises)

Assuming one developer and an available compatible RTX test machine:

| Scope | Estimate | Main uncertainty |
| --- | --- | --- |
| Reproduce existing worker on Windows | 2-5 working days | Runtime compatibility and actual output quality |
| Windows experimental FI stage, 2x SDR | 1-2 weeks after reproduction | Protocol, history, cancellation, frame counts |
| Windows experimental SR/NR stage | 1-3 weeks after reproduction | Carrier, negotiated sizes, fallback reporting |
| Combined Windows product integration | 4-8 weeks total | Packaging, scheduling, recovery, regression coverage |
| Linux/Wine support | Additional 2-4+ weeks, no success guarantee | Driver/translation-layer and unattended runtime behavior |
| Independently maintained native implementation | 2-3+ months research budget | Missing worker source, graphics interop, SDK suitability |

These scopes overlap and should not be added mechanically. A native Linux
implementation remains an unresolved feasibility question, not just a larger
porting estimate.

## Recommended next experiment

Run standalone 2x FI and 2x upscale on existing real animation footage before
changing production architecture. Compare with current RIFE/SR using the same
input, dimensions, FPS, and encoder. Include pans, thin lines, subtitles, scene
cuts, repeated animation frames, and occlusion. Record wall time, GPU memory,
temporal artifacts, fallback flags, output counts, duration, and audio sync.
Require explicit capability failure rather than silently changing algorithms.

For video super-resolution specifically, evaluate NVIDIA RTX Video SDK as a
separate candidate: its official page lists SR, artifact reduction, native CUDA
in SDK 1.1, and Windows 10+ requirements. It is not DLSS FG and does not solve
native Linux support. Prefer evaluating this purpose-built video API before
committing to the NR carrier for faithful video restoration.

Redistribution needs a separate check of the bundled NVIDIA licenses; the
Python project's MIT license alone is not sufficient evidence for all binaries.

## Verification for this documentation-only change

```bash
git diff --cached --check
python /home/zhixi/.codex/skills/secret-guard/scripts/scan_secrets.py staged
```

Expected: no whitespace errors and no detected secrets. No application tests
or builds are needed because no executable project files changed.
