# Windows 720p→4K workflow user report — preliminary analysis 2026-09-28

Source: a Windows v0.1.5 user (RTX 5090, 720p anime, target 3840x2160 with
libx265 slow CRF 16 10-bit) shared a Codex session; saved locally as the
gitignored `.omo/tmp-issue.md`. Analysis was done on `dev` at 48a6335
(14 commits after v0.1.5; none touch these paths). Nothing was run on Windows.

## Status

- Filed upstream by the reporter: #4 (Windows verbatim-path autocomplete),
  #5 (Rescale rejected by VideoCompileContext), #6 (Windows bundle lacks
  libx264/libx265).
- 2026-09-28: items 1-3 below fixed on `dev` (see
  `output-scaling-video-jobs-2026-09-28.md`); #4 and #6 deferred until a
  Windows environment is available.

## Confirmed from code / artifacts

1. **Resize / Rescale cannot run in the video pipeline.**
   `VideoCompileContext::create_processor` (crates/core/src/nodes/compile_context.rs)
   only accepts `SuperResolution` and bails with
   `unsupported processor node '<type>' in VideoCompileContext`. The nodes are
   registered (registry.rs), listed in the editor palette (descriptor.rs) and
   used by preview (server/preview.rs), so the UI accepts graphs that the job
   runner rejects. The user hit this exact error with Rescale.
   `ResizeNode`/`RescaleNode::process_frame` only handle 8-bit `Frame::CpuRgb`,
   while the SR stage can emit `NchwF32/NchwF16` or direct RGB; algorithms are
   only bilinear/nearest (no Lanczos/bicubic).

2. **VideoOutput `width`/`height`/`fps` are ignored in the video pipeline.**
   `VideoCompileContext::encoder_config` takes size/fps from context state
   (decoder size × SR scale, FI multiplier) and never reads those inputs;
   `-s WxH` in `EncoderConfig::build_ffmpeg_args` describes the rawvideo pipe
   input, and the `-vf` chain has no `scale`. The node still declares the three
   ports as `required`, so users reasonably expect them to set output size.

3. **SR `scale` is not validated against the model's native scale.**
   `SuperResDimensions::new` and the tile stitcher in nodes/super_res.rs
   (`out_y0 = y * shape.scale`, crop offsets, output buffer) trust the user
   value. A 4x model with `scale=3` + tiling likely stitches wrong regions
   silently; the non-tiled path likely fails on shape mismatch. Inferred from
   code, not reproduced.

4. **Windows path picker breaks on `\\?\` verbatim paths.**
   `browse_fs` (crates/core/src/server/mod.rs) returns
   `entry.path().canonicalize()`, which on Windows yields `\\?\G:\...`.
   `PathAutocomplete.tsx` then appends `/` for directories; in a verbatim path
   `/` is not a separator, so `exists()` is false and the listing is empty.
   The component also splits only on `/`, so backslash input gets no prefix
   filtering. `dunce` is already in Cargo.lock (transitive), usable to return
   non-verbatim paths. Other canonicalize sites in server/mod.rs and
   server/files/path.rs may leak the same form.

5. **Bundled Windows FFmpeg lacks libx264/libx265.**
   `misc/bin_win64.zip` (128467312 bytes, uploaded 2026-02-22) contains BtbN
   `N-122942-gc7b5f1537d-20260222`, configured with
   `--disable-libx264 --disable-libx265` (LGPL build). All presets default to
   `libx265`, so default Windows jobs are expected to fail at encoder start;
   NVENC (`hevc_nvenc`, `av1_nvenc`) and `libsvtav1` are present.
   `misc/bin_linux64.zip` (updated 2026-09-07) is `n8.1.2` with `--enable-gpl
   --enable-libx265` and lists libx264/libx265 encoders, so this is
   Windows-only. The 2026-09-07 Windows media-tool audit checked PE imports
   only, not encoder availability.

## Suggested fix directions (not implemented)

- Replace the Windows bin asset with a GPL build (and add a packaging check
  that `ffmpeg -encoders` lists every codec the UI offers), or probe encoder
  availability at job validation and fail with an actionable message.
- Implement output scaling in the encoder (`scale=W:H:flags=lanczos` in the
  existing `-vf` chain, driven by VideoOutput width/height or by trailing
  Resize/Rescale nodes), avoiding a CPU 8-bit resize after SR.
- Reject Resize/Rescale at workflow validation until supported, instead of at
  run time.
- Validate SR `scale` against the model's first inference output shape.
- Return non-verbatim paths from browse endpoints (`dunce::canonicalize`) and
  make PathAutocomplete separator-aware for Windows.
