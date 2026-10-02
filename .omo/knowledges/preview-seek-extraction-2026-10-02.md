# Preview extraction by per-sample seek (v0.1.9, branch `feat/preview-seek`)

## Problem

`POST /api/preview/extract` ran one ffmpeg with
`select='not(mod(n\,interval))'`. That decodes every frame up to the last
sample (~90% of the file), so long 4K sources exceeded the 120 s
`EXTRACTION_TIMEOUT`.

## Change

- `PreviewProbe.duration_seconds`: stream duration, else container duration
  (positive, finite), from the existing metadata-only ffprobe.
- With a duration: `preview_sample_times(duration, count)` gives
  `duration * k / count` (same spacing as frame selection), and each sample
  runs `ffmpeg -ss t -i <src> -map <stream> -vf <scale> -frames:v 1 -update 1
  frame_{k+1:04}.png`, 4 at a time (`PREVIEW_SEEK_CONCURRENCY`), all inside the
  same 120 s deadline. Input `-ss` seeks the demuxer to the previous keyframe,
  so cost is bounded by GOP length, not by position.
- A failed sample is skipped and its partial file removed; the request fails
  only when no frame was written. `process_frame` still maps index `i` to
  `frame_{i+1:04}.png`, and the response lists only existing files.
- Without a duration the old `select` single pass remains the fallback.
- `preview_scale_filter` is shared by both paths, so RGB conversion matches.

## Measurement (dev VM, 2026-10-02, release `bin/ffmpeg`, 10 samples)

`1.mkv`, 1080p HEVC, 1422.9 s, 23.976 fps:

| Path | Time |
| --- | --- |
| seek x10, 4 parallel | 3.7 s |
| select single pass | 78.5 s |

At 4K (4x the pixels) the select pass would far exceed 120 s. The local
`1_4k72.mkv` is truncated (58 MB, 127 packets while metadata says 1422.9 s),
so it cannot be used for this benchmark; both paths return 1 frame on it.

## Tests

- `preview_probe_reports_the_duration_to_seek_in`,
  `preview_samples_are_spaced_like_frame_selection`,
  `preview_seek_reads_one_frame_after_an_input_seek` (argument shape).
- Ignored (needs ffmpeg with libx264), run with `--include-ignored`:
  `preview_extraction_seeks_to_evenly_spaced_samples` (red 4 s then blue 4 s,
  samples at 0/2/4/6 s must be red, red, blue, blue) and the existing
  `preview_extraction_reads_frames_from_the_stream_jobs_decode`.
