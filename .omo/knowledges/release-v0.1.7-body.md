## What's new

- **Faster video jobs:** the Worker reuses per-frame buffers through a per-job pool instead of allocating and freeing hundreds of megabytes for every frame. Frame interpolation feeds the GPU from a reusable CUDA-pinned buffer. On an A40 (1080p 120-frame source, 4K output, cached TensorRT engines):

  | Pipeline | v0.1.6 | v0.1.7 | Change |
  |---|---:|---:|---:|
  | Super-resolution → frame interpolation, NVENC | 23.6 s | 14.5 s | −38% |
  | Super-resolution → frame interpolation, libx265 | 25.4 s | 20.1 s | −21% |
  | Super-resolution only | 10.5 s | 9.4 s | −10% |

  Kernel time drops by about 75% (47.7 s → 11.1 s with NVENC), and page faults drop from about 20M to about 3M per job.

## Fixes and reliability

- **Output colours:** `VideoOutput` converted RGB to YUV with the BT.601 matrix but tagged the stream as BT.709, so players showed shifted colours. For example, pure green (0,200,0) played back as (0,167,0). `VideoOutput`, `StreamOutput` and previews now convert with BT.709 in limited range and tag the range.
- **Untagged sources:** sources without a colour matrix tag are decoded with the convention players use: BT.709 when the frame is at least 1280 wide or more than 576 high, BT.601 otherwise. Previously every untagged source was decoded as BT.601, which skewed HD sources. Tagged sources use their tag.
- **Decoder timing:** the decoder summary log now reports the real `avg_decode_ms` instead of 0.0.

## Downloads and Docker

Linux and Windows Worker bundles and standalone Controller archives are available below. Download all parts of a split Worker bundle before extracting the `.7z.001` file.

- Worker: `controlnet/videnoa:0.1.7`
- Controller: `controlnet/videnoa-controller:0.1.7`

Both image repositories also publish `latest`. The Controller does not require a GPU.

## Upgrade

Back up Worker configuration and Controller data before upgrading. Keep the same persistent directories when replacing containers.

- Output colours differ from v0.1.6 because they are now correct. Earlier outputs were shifted, most visibly in saturated greens and reds. Re-run jobs where accurate colour matters.
- Untagged HD sources now decode as BT.709 instead of BT.601.
- Frame interpolation jobs use about 0.3–0.5 GB more host memory, because the buffer pool keeps frames for reuse.

See the [Worker and Docker instructions](https://github.com/ControlNet/videnoa/blob/v0.1.7/README.md), [Controller archive and Docker guide](https://github.com/ControlNet/videnoa/blob/v0.1.7/README-controller.md), and [Controller reference](https://github.com/ControlNet/videnoa/blob/v0.1.7/docs/controller.md).

**Full changelog:** https://github.com/ControlNet/videnoa/compare/v0.1.6...v0.1.7
