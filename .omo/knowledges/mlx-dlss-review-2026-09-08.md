# MLX-DLSS cross-platform review (2026-09-08)

Inspected `iamwavecut/MLX-DLSS` revision
`4549ce6d99837e4fb182c38a028d8e96bc28c0df`. Source/documentation review only;
no vendor binaries, extracted weights, or model inference were executed.

## Mechanism

This project reconstructs NVIDIA NR and FG computation in PyTorch and Swift
MLX/Metal. It does not execute the NVIDIA NGX library at inference time. Users
supply particular NVIDIA libraries; local tooling extracts weights and converts
packed layouts into safetensors or Apple model packages. NR supports a specific
`nvngx_dlssnr.dll` 310.8.0.0 build; FG extraction targets
`libnvidia-ngx-dlssg.so.310.7.0`. Extraction tables are version-dependent.

The author's recovery method uses vendor kernel traces, PTX inspection, and
intermediate-tensor comparisons. The NR implementation explicitly reconstructs
FP8/FP16 rounding, approximate softmax, and noise behavior. This is recovered
network inference with vendor-derived weights, not a newly trained generic
model with DLSS branding, and not a complete official DLSS replacement.

## Actual feature scope

- NR: image/video appearance enhancement, including temporal history. Optical
  flow reprojection, confidence rejection, and scene resets adapt it to video;
  some of these are explicitly project heuristics rather than recovered NVIDIA
  behavior.
- FG: two-stage convolutional synthesis predicts bidirectional flows and masks,
  then warps/blends full-resolution frames. `interpolate(a, b, t)` and batched
  phase evaluation support intermediate frames, FPS multiplication and slowmo.
- SR: deliberately NOT ported. The author reports that tested vendor SR on
  finished media with missing/estimated renderer inputs underperformed Lanczos.
  This is evidence for their test conditions, not a universal SR quality claim.
- `processing_scale` is NR working resolution: Python `prepare()` resamples up,
  then `finish()` resamples back to the original dimensions before detail/color
  composition. It must not be described as DLSS SR or output-resolution scaling.

FG recovery targets a restricted video configuration with zero engine motion
and flat depth. Motion/depth preprocessing, HUD/UI composition, and disocclusion
inpainting are omitted for that configuration. The learned synthesis still
predicts optical flow internally. Do not confuse zero external motion vectors
with absence of motion estimation or with full game-engine equivalence.

## Linux and other platforms

The Python package depends on PyTorch, NumPy, safetensors, and Pillow, with
OpenCV for video temporal processing. `resolve_device()` supports CUDA, MPS,
and CPU. Linux therefore has a native PyTorch/CUDA or CPU route without Wine,
D3D12, ReShade, or an NVIDIA NGX GPU-generation gate. Actual accelerator support
and performance depend on the PyTorch build and operations; AMD/ROCm and other
devices must not be promised without validation.

MLX/Metal targets Apple Silicon macOS. Core ML export is available, but export
on Linux is not evidence that Core ML inference runs on Linux. CI config includes
CPU Python package tests on Ubuntu, Windows, and macOS, plus Apple-specific
verification. This establishes intentional cross-platform coverage, not a real
Linux CUDA parity/performance certification.

## Evidence limits and relevance to videnoa

The author reports FG output matching the vendor video path at 59.9 dB PSNR
(maximum 3/255 error), and NR around 0.004-0.005 MAE on selected game renders.
These measure vendor-output agreement, not quality against ground-truth frames.
Dynamic NR temporal vendor parity remains less validated than static sequences.
Local review did not reproduce any accuracy or performance numbers.

Author FG timing: M2 Max, 1080p, 2x, one pair, about 16.58 ms for warmed Metal
synthesis/composition, excluding startup and I/O, using real weights with
synthetic RGB inputs. This upstream benchmark uses synthetic imagery; it is not
a local real-video benchmark or a Linux CUDA speed claim. NR is much heavier:
README reports roughly 8 s per 1440x1280 frame for the MPS reference graph.

For videnoa, this is a plausible Python/CUDA experimental backend using the
existing frame-pair boundary, avoiding Vulkan worker development. Start with FG
quality/throughput on real animation clips and an existing RIFE baseline.
Safetensors are weights, not an ONNX executable model. No ready ONNX export path
was found in the inspected Python/docs tree. ONNX/TensorRT integration would
need separate export and parity work, including GridSample, shapes, and rounding.

Source is Apache-2.0; the README says vendor-derived model packages remain subject
to vendor licensing and no weights are included. Do not infer weight
redistribution rights from the source-code license.

## Sources

- [README](https://github.com/iamwavecut/MLX-DLSS/tree/4549ce6d99837e4fb182c38a028d8e96bc28c0df)
- [FG recovery and validation](https://github.com/iamwavecut/MLX-DLSS/blob/4549ce6d99837e4fb182c38a028d8e96bc28c0df/docs/frame-generation.md)
- [SR scope](https://github.com/iamwavecut/MLX-DLSS/blob/4549ce6d99837e4fb182c38a028d8e96bc28c0df/docs/super-resolution.md)
- [PyTorch FG](https://github.com/iamwavecut/MLX-DLSS/blob/4549ce6d99837e4fb182c38a028d8e96bc28c0df/python/mlxdlss/framegen.py)
- [NR pipeline and device selection](https://github.com/iamwavecut/MLX-DLSS/blob/4549ce6d99837e4fb182c38a028d8e96bc28c0df/python/mlxdlss/pipeline.py)
- [CI matrix](https://github.com/iamwavecut/MLX-DLSS/blob/4549ce6d99837e4fb182c38a028d8e96bc28c0df/.github/workflows/ci.yml)

Documentation verification: `git diff --cached --check` and
`python /home/zhixi/.codex/skills/secret-guard/scripts/scan_secrets.py staged`.
Expected: no whitespace errors or detected secrets. No application build or
tests were needed for this documentation-only change.
