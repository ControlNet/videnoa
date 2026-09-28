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

## Separate finding (not fixed): ESRGAN output is much darker

Mean RGB of 64×36 area-downscaled frames on a real 1080p anime clip scaled to
640×360:

| Pipeline | Mean RGB |
| --- | --- |
| source | 41.6 / 40.6 / 50.8 |
| ESRGAN 4x, tile 320 | 18.3 / 18.7 / 24.4 |
| ESRGAN 4x, tile 0 | 18.3 / 18.7 / 24.5 |
| AnimeJaNai 2x fp16 | 39.8 / 38.9 / 49.6 |

A synthetic clip gave 122.8→65.7 mean red. Output scaling alone keeps
brightness, so the cause is in the ESRGAN / fp32 path, independent of
tiling. The fp32 path feeds 0–255 and clamps 0–255 output; the fp16 path uses
0–1. This affects the reporter's `RealESRGAN_x4plus_anime_6B` workflow and
needs its own investigation.
