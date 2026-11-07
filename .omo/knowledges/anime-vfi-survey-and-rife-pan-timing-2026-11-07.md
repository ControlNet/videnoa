# Anime VFI/SR survey and RIFE pan timing test (2026-11-07)

Context: a user reported that `presets/interpolation-3x-anime-2x.json`
(RIFE v4.26 3x, then AnimeJaNai V3 L1 2x) still looks juddery on anime pans.

## Measured: RIFE 4.26 puts 3x frames at uneven positions on flat line art

Test setup (synthetic, ground truth known):

- Canvas: flat fills, 2 px dark grid lines, outlined boxes, text (anime-like).
  A second canvas used the ffmpeg `mandelbrot` source (dense texture).
- Ground truth: a 1920x1080 crop panned 8 px per frame at 72 fps (216 frames).
  The source is every 3rd frame at 24 fps (24 px per source frame), x264 `-qp 0`.
- Ran Videnoa (`videnoa run`) with an interpolation-only copy of the preset,
  output `libx264` `crf 0`, backends `cuda` and `tensorrt`.
- For each output frame, a sub-pixel search over canvas crops found the pan
  offset that best explains it (luma MSE), giving the frame's effective t.

Results on the flat line-art canvas (source frames k=33 and 34, x=792 and 816):

| Requested t | Effective t (CUDA EP) | Effective t (TensorRT EP) | Effective t (ORT CPU) |
|---|---|---|---|
| 1/4 (4x) | 0.183 | not run | not run |
| 1/3 | 0.267 | 0.267 | 0.267 |
| 1/2 (2x) | 0.517 | not run | 0.517 |
| 2/3 | 0.733 | 0.733 | 0.733 |
| 3/4 (4x) | 0.817 | not run | not run |

- 3x steps become 0.267 / 0.467 / 0.267 of a source interval instead of 1/3 each,
  so pan speed alternates by about 1.75x inside each 24 fps interval. That reads
  as judder.
- Interpolated-frame luma PSNR vs ground truth was about 25 dB (real frames: lossless).
- On the textured canvas, the same run landed exactly on t (x=800, 808) at about 45 dB.
  The error is content dependent: flat colour plus thin lines, which is typical of
  anime cels and simple backgrounds.
- TensorRT vs CUDA EP: same positions, PSNR within about 0.5 dB. The VSGAN-tensorrt-docker
  warning that "Rife with TensorRT is broken ... wobbly lines and artefacts during
  panning scenes" was about torch_tensorrt's grid_sample conversion (vs-rife commits
  0168d7e, 4d6ff75). It did not reproduce through ORT's TensorRT EP with the bundled model.
- An independent ONNX export of the same weights (ComfyUI-RIFE-ONNX `rife_v4.26_comfy.onnx`,
  3-input img0/img1/timestep[batch]) was also off (0.350 / 0.567 / 0.808). Its padding and
  input conventions are unverified, and its `heavy` export gave broken output (15.6 dB)
  under the same feed, so treat those numbers as indicative only.
- Videnoa's 3-input path feeds `timestep` as a 1x1x1x1 tensor, so the ComfyUI exports
  (`timestep` shape `[batch]`) are not drop-in.

Caveat: this is one synthetic scene. It has not yet been measured on a real anime pan.

Also noted: `is_scene_change` is never set in the streaming executor, so
FrameInterpolation always interpolates across cuts. The user does not consider this a problem.

## Survey: interpolation models used for anime

All quality claims below are from forums and READMEs. No head-to-head benchmark was found.

- **RIFE 4.x (Practical-RIFE, MIT).** The common default. It is fast and has ONNX/TRT exports.
  The SVFI docs call 4.8 strongly anime-biased. 4.26 heavy is a quality variant.
- **GMFSS Fortuna / GMFSS Union (MIT, 98mxr; HolyWu vs-gmfss_fortuna, MIT, last push 2023).**
  The community rates it the best for anime, but it is slow. It is built on GMFlow plus
  softmax splatting (CuPy kernels), and no ONNX export was found, so it is not a drop-in for ORT.
- **DRBA (hyw-dev/DRBA, MIT).** A control layer over RIFE/GMFSS. It computes a distance-ratio
  map so that nonlinear (character) regions keep their original pace. It is PyTorch, and
  VS-DRBA uses torch_tensorrt. It needs three frames plus GMFlow.
- **MultiPassDedup (MIT).** Anime dedup by repeatedly updating original frames rather than
  identifying duplicates. It pairs with GMFSS.
- **Dedup in general.** The Flowframes docs recommend it for 2D animation. Known failures:
  false positives in dark or low-contrast scenes, long holds collapsed into slow morphs,
  and mixed cadences. The SVP dev says SVP only handles the "every other frame" case.

SR: AnimeJaNai V3 (current) is the community default for anime HD to 4K. Alternatives that
come up are ModernSpanimation V2 (SPAN) and Real-CUGAN (slow).

## Sources

- https://github.com/n00mkrad/flowframes (README/FAQ, de-duplication)
- https://github.com/hyw-dev/DRBA
- https://github.com/routineLife1/MultiPassDedup
- https://github.com/HolyWu/vs-gmfss_fortuna
- https://github.com/styler00dollar/VSGAN-tensorrt-docker (RIFE TensorRT warning)
- https://github.com/Austat/ComfyUI-RIFE-ONNX
- https://www.svp-team.com/forum/viewtopic.php?id=6598
