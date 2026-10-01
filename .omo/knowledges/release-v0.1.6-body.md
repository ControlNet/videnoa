## What's new

- **Output scaling in video jobs (#5):** `Resize` and `Rescale` now work as the last processing nodes before `VideoOutput`. FFmpeg applies them in the final encode pass, so there is no extra intermediate encode. `SuperResolution (4x) → Rescale (0.75) → VideoOutput` now turns 1280×720 into 3840×2160. `VideoOutput` `width`/`height` are now optional, and when set they resize the output.
- **Lanczos resampling:** `Resize` and `Rescale` add a `lanczos` algorithm and use it by default for new nodes. It is sharper than bilinear for downscaling after super-resolution.
- **Checks before a job is queued:** unsupported graphs are rejected with a message that explains the rule, for example a processing node after `Resize`/`Rescale`. So is an output encoder that the bundled FFmpeg does not provide (#6). The Web UI returns HTTP 400 and `videnoa run` fails at validation, instead of the job failing after it starts.
- **Super-resolution scale check:** a `SuperResolution` `scale` that does not match the model's native scale now fails immediately and suggests the fix. Previously it produced wrong output or crashed.
- **Real previews:** the single-frame preview runs the actual SuperResolution, Resize and Rescale processors on the extracted frame. Preview files live in bounded sessions and are cleaned up when released, when they expire, and at shutdown.

## Fixes and reliability

- **RealESRGAN brightness:** FP32 RealESRGAN models received 0–255 input instead of 0–1, so the output was about half as bright as the source. They now match the source brightness.
- **Clean CLI exit with CUDA:** `videnoa run` no longer aborts with `corrupted double-linked list` after "Workflow completed successfully". The output was complete before, but the exit code was wrong.
- **Windows encoders (#6):** the Windows bundle now ships the BtbN FFmpeg **n8.1.3 GPL** build, which includes `libx264` and `libx265`. The previous LGPL build could not run the default `libx265` output.
- **Windows path autocomplete (#4):** directory browsing returns plain `G:\...` paths instead of `\\?\` paths. The autocomplete understands `\`, so picking folders stays navigable, including folders with spaces. File API paths are also correct when the workspace is on a different drive.
- **Job correctness:**
  - A decoder that fails or emits a partial frame now fails the job.
  - Cancelling a job stops scalar and nested workflows between steps.
  - Path parameters keep their declared type.
  - Deleting job history can no longer resurrect or cancel a job when the delete fails.
- **Controller:**
  - The live event stream recovers after the machine sleeps.
  - Paths are reopened after a data-root replacement.
  - Publication falls back to copying when a Linux no-replace rename returns `EINVAL`.
  - The task view reads global counts once per view instead of on every page change.
- **Release checks:** CI and release packaging now check that the bundled FFmpeg lists every encoder the UI offers. They also run real x264/x265 encodes through the VideoOutput filter chain, with MKV statistics tagging. Windows CI now reports every Rust test failure; before, the shell masked failures from all but the last command.

## Downloads and Docker

Linux and Windows Worker bundles and standalone Controller archives are available below. Download all parts of a split Worker bundle before extracting the `.7z.001` file.

- Worker: `controlnet/videnoa:0.1.6`
- Controller: `controlnet/videnoa-controller:0.1.6`

Both image repositories also publish `latest`. The Controller does not require a GPU.

## Upgrade

Back up Worker configuration and Controller data before upgrading. Keep the same persistent directories when replacing containers.

- `VideoOutput` no longer has an `fps` port; the source frame rate is always preserved. Saved workflows with an `fps` value keep working, because the value is ignored. Only a connection into `fps` now fails validation.
- A `SuperResolution` node whose `scale` differs from the model's native scale now fails instead of producing wrong output. Set `scale` to the native scale, for example 4 for `RealESRGAN_x4plus_anime_6B`. Then reach the target size with `Rescale`/`Resize` or with `VideoOutput` `width`/`height`.
- Output from FP32 RealESRGAN models is brighter than in v0.1.5, because it is now correct. Re-run jobs whose results looked too dark.
- Windows v0.1.5 users who cannot upgrade yet can replace the install's `bin` folder with [`bin_win64.zip`](https://github.com/ControlNet/videnoa/releases/download/misc/bin_win64.zip). See #6 for the steps.

See the [Worker and Docker instructions](https://github.com/ControlNet/videnoa/blob/v0.1.6/README.md), [Controller archive and Docker guide](https://github.com/ControlNet/videnoa/blob/v0.1.6/README-controller.md), and [Controller reference](https://github.com/ControlNet/videnoa/blob/v0.1.6/docs/controller.md).

**Full changelog:** https://github.com/ControlNet/videnoa/compare/v0.1.5...v0.1.6
