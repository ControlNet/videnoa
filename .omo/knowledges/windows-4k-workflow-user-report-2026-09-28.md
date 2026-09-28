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
  `output-scaling-video-jobs-2026-09-28.md`).
- #4 was fixed on `dev` in 0ff5da1 (see below), verified by the Windows CI
  runner only.
- #6: `misc/bin_win64.zip` was replaced with a verified GPL bundle on
  2026-09-28 (see below). Existing release archives still contain the old one.
- 2026-09-28: posted a status comment on #5 (issuecomment-5863542114). It
  covers the output-scaling fix (d8ef723), the RealESRGAN brightness fix
  (25dc102) and the CUDA exit-abort fix (35fb337), and asks the reporter to
  confirm on TensorRT.
- 2026-09-28: posted status comments. #4: issuecomment-5867257826 (fix and
  CI-only verification). #6: issuecomment-5867258292 (asset replacement, smoke
  checks, pre-check, and a PowerShell workaround that swaps the v0.1.5 `bin`
  folder). #5: issuecomment-5867258694 (the TensorRT re-run of the exact
  graph). All three stay open until v0.1.6 ships.
- The fixes are on `dev` only. The maintainer chose not to merge to `master`
  yet, so #5 stays open until that merge ("Fixes #5").
- A push to `master` runs `release.yaml`, which skips publishing while tag
  `v0.1.5` exists. A new release therefore needs a version bump.

## #4 fix (2026-09-28, commit 0ff5da1)

- `browse_fs` now uses `dunce::canonicalize` for the browsed directory and
  its entries, so it returns plain `G:\...` paths. `dunce` keeps `\\?\` only
  when it is required.
- `repair_verbatim_separators` rewrites `/` to `\` after a `\\?\` prefix.
  This repairs values that older UIs saved.
- `server/files.rs::workflow_response_path` runs `dunce::simplified` on both
  sides before `pathdiff`. Without that, the Windows file API returned
  absolute paths.
- The web client adds `components/shared/path-input-utils.ts`
  (`splitPathInput`, `withTrailingSeparator`). It splits on the last `/` or
  `\`, and appends the separator the path already uses.
- The Windows-only test `test_browse_fs_accepts_verbatim_input_and_returns_plain_paths`
  covers the browse fix. It could not be compiled locally: cross-checking for
  `x86_64-pc-windows-msvc` fails in the blake3/libsqlite3 C builds.

## Windows CI masks Rust test failures (found 2026-09-28, fixed in c0907cd / 3aba87b)

- The `rust-tests` step in `unittest.yaml` runs three `cargo test` lines. On
  `windows-latest` the default shell is pwsh, which only reports the last
  command's exit code. So core failures show as a green job.
- Because cargo stops after the failing lib tests, core's integration tests
  never ran on Windows.
- Failures at 35fb337:
  - the files stat and upload tests (non-relative response path; fixed by
    0ff5da1);
  - files outside-workspace (the test used `strip_prefix("/")`);
  - `test_browse_fs_tilde`;
  - `test_fs_list_traversal_blocked` (200: `/etc` is missing, so the check
    was never reached; not a traversal hole);
  - `test_performance_routes_use_enabled_envelopes_when_profiling_is_enabled`.
    RAM metrics read `/proc/meminfo` only, so the Windows performance panel
    has no RAM data. This one is not fixed.
- Fixed in c0907cd: the step uses `shell: bash`, and the workflow contract
  requires it. The RAM assertions only run on Linux; the maintainer chose not
  to support Windows RAM metrics for now.
- The first unmasked run (36380174043) still failed the files stat and upload
  tests. On runners the temp dir is on `C:` and the checkout on `D:`, so no
  relative path exists. Fixed in 3aba87b: `relative_to_or_absolute` in
  `server/files.rs` returns the plain absolute path when the first path
  component (the drive prefix) differs.

## #6 candidate bundle (2026-09-28, commit 2c0b17c, not published)

- `scripts/media_tools.ps1 -Build -OutputDir <dir>` assembles
  `bin_win64.zip`. It uses BtbN `autobuild-2026-09-26-13-03` /
  `ffmpeg-n8.1.3-win64-gpl-8.1.zip`, with SHA-256
  `d20ef03f0f4161453b9f46a72471eb5370f56b6410fe8bca14a4b0220c6c924a`, pinned
  in the script. mkvpropedit.exe v97.0 is copied from the current asset. The
  zip adds FFMPEG-LICENSE.txt, README-windows-tools.txt and SHA256SUMS under
  the `bin/` root that `package_dist.ps1` expects.
- `-Verify -BinDir <dir>` checks four things:
  - the build flags `--enable-gpl`, `libx264`, `libx265` and `libzimg`;
  - the encoders libx264, libx265, hevc_nvenc and h264_nvenc;
  - the filters;
  - real encodes. Raw rgb24 frames go through VideoOutput's
    `format/setparams/zscale/setsar` chain for x264 and x265 at 8 and 10 bit.
    Each output then runs `mkvpropedit --add-track-statistics-tags`, and
    ffprobe must count 30 frames and a NUMBER_OF_FRAMES tag of 30.
  - NVENC is only listed, not exercised, because runners have no GPU.
- The script was renamed from `windows_media_tools.ps1`. `-Verify` also runs
  on Linux (no `.exe` suffix) and passes against `misc/bin_linux64.zip`
  (n8.1.2, mkvpropedit 101.0). Both package smoke jobs run it on the
  assembled `videnoa/bin`, and the unittest contract requires that step after
  "Build package bundle". Local run: portable pwsh from the PowerShell GitHub
  release, unpacked into the scratchpad.
- `.github/workflows/windows-media-tools.yaml` runs on dev/master pushes that
  touch the script or workflow, and on workflow_dispatch. It uploads the
  artifact `bin_win64-candidate` and keeps it for 14 days.
- Run 36380931379 passed on the first try. The candidate zip is
  137106145 bytes with SHA-256
  `f972bd46f396ef3872c99fa7b23867b402c1938b8abd5264080d82825a0fd4e1`. Both the
  staged `bin/` and the zip re-extracted from it were verified.
- **Published 2026-09-28 05:28 UTC** with maintainer approval. The
  maintainer ran `gh release upload misc <candidate> --clobber` themselves,
  because auto mode blocks asset overwrites. The file was then re-downloaded
  from the public URL and matched the candidate by SHA-256 and `cmp`.
  Application release archives were not rebuilt.
- Backup of the old asset: `~/.local/share/videnoa/release-asset-backups/2026-09-28/bin_win64.zip`.
  It is 128467312 bytes with SHA-256
  `fdc08e43dc7aaabf6eed1c1fe5c9b107e13ba37d6e1e2f5c421600d7223a34a6`, and it
  is the BtbN LGPL build `N-122942-gc7b5f1537d-20260222`. To roll back, run
  `gh release upload misc <backup> --clobber`.
- Encoder pre-check: `nodes/encoder_availability.rs::validate_workflow_encoders`.
  - It runs after `validate_video_workflow` in the server's
    `parse_and_validate_workflow` (HTTP 400) and in the CLI validate step.
  - It collects the `codec` of each VideoOutput (default libx265) and
    StreamOutput (default libx264). A `codec` fed by a connection is skipped,
    since it is only known at run time.
  - It compares them with `ffmpeg -hide_banner -encoders` from
    `runtime::command_for("ffmpeg")`, probed once per process (`OnceLock`).
  - When the probe fails or lists no encoders, the check is skipped so tests
    and unusual builds are not blocked.
  - The error names the node, the missing encoder and the offered encoders
    this FFmpeg does have, and points to GPL builds.
  - `tests/encoder_availability.rs` is its own process. It changes CWD to a
    temp dir whose `bin/ffmpeg` lists only NVENC, expects 400 for the default
    libx265 and 201 for hevc_nvenc. It fails when the server wiring is
    removed.
- Remaining: a release. The v0.1.5 archives still bundle the old LGPL ffmpeg.

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

## Pre-release verification (2026-09-28, dev a6ffa71 + release.yaml fix)

- #5 with the issue's exact graph and the TensorRT backend on the A40, using
  `target/release/videnoa run`. Input: 2 s, 1280x720 at 24000/1001, ffv1 plus
  vorbis audio. Graph: WorkflowInput -> VideoInput -> SR (RealESRGAN x4plus
  anime 6B, scale 4, tile 320, tensorrt) -> Rescale 0.75 bilinear ->
  VideoOutput (libx265 slow, crf 16, yuv420p10le, 3840x2160).
  - A cold engine build took about 4 min; 48 frames then ran at 1.8 fps.
  - Exit 0, with no abort after "Workflow completed successfully".
  - ffprobe: 3840x2160, HEVC Main 10, bt709 tv range, 24000/1001, 48 frames,
    NUMBER_OF_FRAMES=48, audio kept.
  - Mean luma after downscaling to 720p is 127.09 against the source's 125.69,
    so there is no darkening. PSNR against the source is 35.74 dB, so there is
    no tile misplacement.
- #4 has no remaining `\\?\` leak on the server.
  - `browse_fs` and the files API use `dunce`.
  - `fs/list` returns `base/relative` display paths.
  - `PathAutocomplete` is the only consumer of `/api/fs/browse`, and it uses
    the separator-aware helpers.
  - This was verified by Windows CI and vitest only, not by clicking through
    a real Windows UI.
- #6: the release package jobs now also run `media_tools.ps1 -Verify`, and the
  Windows release `7z a` step checks `$LASTEXITCODE`; the release contract
  requires both. The Docker image uses Ubuntu 22.04's apt ffmpeg 4.4, which is
  GPL with x264, x265, zimg and nvenc. The same package passed `-Verify`
  locally.
