# Media pipeline audit fixes (2026-10-01)

Fixes for the encoder/decoder/preview findings of `project-audit-2026-10-01.md`.

## FFmpeg / NVENC facts verified on this host

- NVENC on the A40 rejects frames below 144x144: a 64x64 probe fails with
  "Frame dimensions are less than the minimum supported value". The probe
  now encodes 256x256 (`NVENC_PROBE_SIZE`), has a 30 s deadline and memoises
  successful probes per (codec, pixel format, preset, profile). Failures are
  not memoised.
- FFmpeg 5+ (bundled 8.1) stamps `scale=out_color_matrix=...` on the frame
  and libx264 writes it to the VUI, so a clip made with only `scale=...` is
  tagged `color_space=bt709`. FFmpeg 4.4 leaves it untagged. To make a truly
  untagged fixture append
  `setparams=colorspace=unknown:color_primaries=unknown:color_trc=unknown`.
- Without `-pix_fmt`/`format=`, libx264 fed an RGB pipe negotiates 4:4:4
  (yuv444p / yuv444p10le). StreamOutput now forces `yuv420p`
  (`STREAM_PIXEL_FORMAT`).
- FFmpeg's automatic video stream choice prefers the default-disposition
  stream (8.1) or the largest one, not `v:0`. Preview extraction maps `0:v:0`
  to match its `-select_streams v:0` probe. Jobs pick the primary stream with
  `select_primary_video_stream` (default, non-attached-picture first), so a
  file whose default video stream is not the first one previews a different
  stream than the job encodes.
- `ffprobe -show_entries stream=nb_frames,...:format=duration` (no
  `-count_frames`): MP4 reports `nb_frames` and stream `duration`; Matroska
  reports neither, only `r_frame_rate` and `format.duration`.
- YUV->RGB with `flags=bicubic` drifts up to ±3 per channel on a 4:2:0 round
  trip; `bicubic+accurate_rnd+full_chroma_int` keeps it within ±2 under both
  FFmpeg 8.1 and 4.4 (`RGB_DECODE_SCALE_FLAGS`).

## Subprocess helpers (`crates/core/src/subprocess.rs`)

- `StderrTail`: drains stderr on a thread, logs each line, keeps the last
  4 KiB for error messages. Encoders quote it on non-zero exit and on stdin
  write failure.
- `wait_or_kill`: `try_wait` polling with a deadline, kill and reap on expiry.
- `output_with_timeout`: stdin written from a thread while stdout/stderr are
  drained (no pipe deadlock), killed at the deadline; used by preview
  processing (ffprobe, one-frame decode, PNG encode; 60 s each).
- Test fakes: run fake `ffmpeg` scripts via `sh <script>` rather than
  executing a freshly written file, or concurrent test forks make exec fail
  with ETXTBSY ("Text file busy"). Use `exec sleep` in hung-child fakes so the
  kill reaches the sleeping process and its pipes close.

## Verification commands

```bash
export ORT_DYLIB_PATH=$PWD/lib/libonnxruntime.so
export TRT_LIBS=$HOME/miniconda3/envs/anime/lib/python3.13/site-packages/tensorrt_libs
export LD_LIBRARY_PATH=$TRT_LIBS:$PWD/lib:$HOME/miniconda3/envs/anime/lib:$LD_LIBRARY_PATH
export PKG_CONFIG_PATH=$HOME/miniconda3/envs/anime/lib/pkgconfig:$PKG_CONFIG_PATH
FILTER="decoder_recovers_source_rgb encoded_output_decodes_back first_frame_decode_matches preview_extraction_reads_frames nvenc_probe_passes_on_real_hardware"
PATH=$PWD/bin:$PATH cargo test -p videnoa-core --lib -- --ignored $FILTER   # FFmpeg 8.1
cargo test -p videnoa-core --lib -- --ignored $FILTER                      # FFmpeg 4.4
cargo test -p videnoa-core --lib -- --ignored micro_stages_match_the_single_node_path  # A40
```
