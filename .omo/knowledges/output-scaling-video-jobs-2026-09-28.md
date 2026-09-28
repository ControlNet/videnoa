# Output scaling in video jobs + SR scale validation — 2026-09-28

Fixes issue #5 (Rescale rejected by `VideoCompileContext`) on `dev`.

## Design

- Trailing `Resize`/`Rescale` nodes compile to **no frame stage**.
  `VideoCompileContext::create_stages` records an `OutputScale`
  (`video_output.rs`), and the encoder prepends
  `scale=W:H:flags=<alg>+accurate_rnd+full_chroma_int` before `format=` in
  `-vf`. `-s` stays the piped (pre-scale) size; `EncoderConfig::output_dimensions()`
  is the encoded size. This gives one lossy encode and no CPU resize at 4K.
- After a Resize/Rescale, any SR/FI stage fails with
  "'X' cannot follow Resize/Rescale". `validate_video_workflow` (compile_context.rs)
  applies the same rule, and also rejects processing types the video pipeline
  cannot compile. It runs in `parse_and_validate_workflow` (HTTP 400 before
  queueing) and in CLI `run`. Preview keeps its own rules, so Resize→SR still
  previews.
- VideoOutput `width`/`height` are optional. When both are set and differ from
  the pipeline size, a final lanczos `OutputScale` is added. Setting only one is
  an error. An odd scaled size with a 4:2:0 pixel format is an error. The `fps`
  port was removed: it was never read. Stale `fps` params are ignored because
  `resolve_inputs` iterates port definitions; a *connection* into `fps` would now
  fail validation.
- Resize/Rescale gained `lanczos` (the new default, and the Rust Lanczos-3 used
  by preview). `from_str_lossy` still maps unknown names to bilinear.
- Pipe bit depth: the decoder emits 16-bit `CpuRgb` for sources above 8 bits,
  but the encoder used to hard-code 8. As a result, `VideoInput → VideoOutput`
  on a 10-bit source failed with "frame size mismatch: expected 3072 bytes, got
  6144". This was reproduced on the pre-fix HEAD. The context now tracks
  `pipe_bit_depth` (16 for a >8-bit source until SR/FI, then 8), and the
  FrameSink converts rgb48le↔rgb24 when a frame's depth differs.
- SR scale:
  - `SuperResNode::execute` rejects a `scale` that differs from a built-in
    model's native scale (`model_registry::builtin_model_scale`, matched by file
    name) before the session or TensorRT build.
  - `check_output_scale` in super_res.rs validates every inference output
    (fp32/fp16, tiled/untiled, and the micro-stage `SuperResInference`)
    before cropping or stitching. Scale below native used to corrupt output
    silently; scale above native used to panic on out-of-bounds slices.
  - The web `ModelSelector` sets `scale` from `ModelEntry.scale`
    (`modelSelectionParams`).

## Verification (A40)

- `crates/core/tests/output_scaling.rs` (real `/usr/bin/ffmpeg`): Rescale
  0.5 → 16×16; a 10-bit source with VideoOutput 24×16; Rescale→SR → 400.
  All three fail on the pre-fix HEAD with the original errors.
- The GPU ignored test `mismatched_scale_on_unknown_model_fails_on_first_frame`
  must be run **from the repo root**, because the pre-existing ignored SR tests
  open `models/...` relatively:
  `target/debug/deps/videnoa_core-<hash> nodes::super_res --ignored`
- CLI run of the #5 graph (1 s of 1280×720, ESRGAN 4x tile 320 on the CUDA
  backend, Rescale 0.75 lanczos, x265 slow CRF 16 10-bit, width/height
  3840×2160): hevc 3840×2160 yuv420p10le, 30/30 frames, audio kept.
  `scale=3` fails in 0.2 s with "is a 4x model but scale=3".

## ESRGAN output was much darker (fixed 2026-09-28)

Symptom: with `RealESRGAN_x4plus_anime_6B` the output was roughly half as
bright as the source. Mean RGB of a real 1080p anime clip scaled to 640×360
(the first frame, full resolution):

| Pipeline | Before fix | After fix |
| --- | --- | --- |
| source | 42.3 / 40.3 / 51.4 | same |
| ESRGAN 4x, tile 320 | 18.7 / 18.8 / 24.8 | 40.1 / 39.4 / 49.7 |
| ESRGAN 4x, tile 0 | 18.7 / 18.8 / 24.8 | 40.1 / 39.4 / 49.8 |
| AnimeJaNai 2x fp16 | 40.1 / 39.1 / 49.8 | 40.1 / 39.1 / 49.8 |

Root cause: the fp32 path in `nodes/super_res.rs` (`cpu_rgb_to_nchw_into`,
`nchw_to_cpu_rgb`, and the `NchwF32` branch that multiplied by 255) fed the
model 0–255 and read its output as 0–255. The ONNX graph starts directly with
Conv and has no normalization, and the model is trained on 0–1 like every
Real-ESRGAN export. Tiling and output scaling were not involved.

How it was proven: a CPU onnxruntime run (conda `comfyui`, read-only) on a
real 160×90 crop. The current 0–255 convention gave a mean of 36.7 against a
source of 74.5, with a raw output of 1.4..152. Feeding 0–1 and multiplying by
255 gave 74.4, with a mean absolute difference of 4.99 from the source.

Fix:
- fp32 input is now 0–1. That is ÷255 for 8-bit, ÷65535 for 16-bit, and for
  9–15-bit the u8 quantization then ÷255.
- fp32 output is `(v*255+0.5).clamp(0,255)`.
- `NchwF32` frames are passed through unchanged. They are 0–1, as RIFE
  already emits them.
- The model registry's `normalization_range` for ESRGAN is now `(0,1)`. It is
  display-only, shown on the Models page.

Tests:
- `fp32_pre_and_post_processing_round_trip_every_level` failed before the
  fix.
- The ignored GPU test `upscaling_preserves_mean_brightness` covers both
  bundled models, tile 0 and 64, and CpuRgb and NchwF32 input. It checks the
  mean is within 4 levels.

Remaining minor bias: the fp16 postprocess truncates (`as u8` without +0.5),
about −0.5 levels on average. It is left as is.

## Pre-existing issues seen during verification (not fixed)

- `videnoa run` sometimes aborts after "Workflow completed successfully" with
  glibc `corrupted double-linked list` (exit 134). The output file is already
  complete. It reproduced on unmodified `d8ef723` in 7 of 12 runs, with both
  the fp32 and the fp16 model, so it is a teardown or heap bug at process
  exit, probably ORT/CUDA EP destruction order. Scripts that check the exit
  code will see a failure.
- `server::preview_cache::tests::admission_drop_and_restart_cleanup_are_bounded`
  is flaky when run together with the subprocess tests in the same module
  (about 8 of 10 runs from the repo root). It passes alone or with
  `--test-threads=1`. The lock is freed about 1 ms later and no process holds
  the fd by the time it is checked. So a concurrently spawned child briefly
  inherits the lock's open file description before exec. This is a test race
  only.
