# VideoOutput: AI output "has no audio" because the MKV is not interleaved (2026-10-18)

## Symptom

`装备我最强 S01E01 - AI.mkv` (4K, 71.928 fps HEVC Main10 output of a
1080p 23.976 fps AAC source) plays with no audio in the user's player. The
source file plays fine.

## What the file actually contains

- The audio track is present and bit-identical to the source:
  `ffmpeg -i X -map 0:a:0 -c copy -f hash -hash md5 -` gives the same MD5,
  66567 packets in both, and `volumedetect` gives the same mean/max volume.
- The layout is broken. Use `ffprobe -read_intervals "T%+0.1"` per stream
  and compare packet `pos` values:
  - source: audio and video for t=600 both sit around byte 598M;
  - AI output: every audio and subtitle packet sits at bytes 25M-60M. The
    first video packet is at byte ~60M, and video then runs to 2.1G. For
    example, audio at t=1200 is at 54M while video at t=1200 is at 1.8G.
- Players that read clusters in order (network streaming, Plex/Jellyfin
  direct play, TVs, browsers) never see the matching audio, or lose it
  after a seek.
- Minor quirk, not the cause: the ffmpeg-muxed AAC track carries
  `Bit depth: 32`, which the mkvmerge source does not have.

## Cause: FFmpeg's scheduler, not just the slow pipe

The old `build_ffmpeg_args` ran one ffmpeg process with two inputs:

- input 0 is the processed frames on `pipe:0`, produced at AI speed;
- input 1 is the source, whose non-video streams are copied with `-c copy`.

The FFmpeg 7+ CLI scheduler (`fftools/ffmpeg_sched.c`, `trailing_dts` and
`schedule_update_locked`) lets an input keep reading only while the output
stream it feeds is the one furthest behind (within `SCHEDULE_TOLERANCE`,
100 ms). If any unfinished output stream has no DTS yet, only the sources
of such streams may read.

- **Font attachments** never produce packets, so the shared source demuxer
  read the whole file at once, and `max_interleave_delta` (10 s) flushed
  all of that audio before any video. On a 2-minute real clip, audio landed
  110 s ahead.
- **Sparse subtitle tracks** cause the same read-ahead during gaps in the
  dialogue (about 17 s measured without fonts).
- **Audio tracks of unequal length** do the same once the shorter one ends
  (19 s lead measured with a 30 s track plus a 1 s track).

Approaches that were measured and rejected:

| Approach | Problem |
|---|---|
| `-max_interleave_delta 0` | When a subtitle track ended early, the whole output was buffered in RAM: the file stayed at 0 bytes during the encode. |
| Two-stage, with ffmpeg doing the remux | Same scheduler behaviour: 110 s lead with fonts. |
| Splitting the source into per-type inputs: `-i SRC` for audio, `-t 1 -i SRC` for attachments, `-i SRC` for subtitles (commit bd3a070, reverted in 9d2f100) | Fixed the reported file, but a Codex (gpt-6-astra) review found three problems, two of them reproduced here. |

The Codex findings on the split-input approach:

- **Reproduced:** audio tracks of unequal length still share one demuxer
  and give a 19 s lead.
- **Reproduced:** `-t 1` never stops a packetless attachment input. The
  input-duration check only runs on selected packets, so the attachment
  input read the whole 162 MB clip, and the source was scanned three times.
- **Not reproduced, source-reasoned:** a separate subtitle input does its
  own `correct_input_start_times` and discontinuity handling, so subtitles
  can drift on MPEG-TS and other `AVFMT_TS_DISCONT` sources.

## Fix (ead34f4): mkvmerge muxes Matroska outputs

For `.mkv` and `.mka` outputs, VideoEncoder works in two steps:

1. FFmpeg encodes only the pipe (`build_video_only_ffmpeg_args`, with no
   source input) into a hidden temp file beside the output,
   `.{stem}.XXXXXX.videnoa-video.mkv`.
2. In `finish`, mkvmerge writes a second hidden temp file,
   `.{stem}.XXXXXX.videnoa-mux.{ext}`:
   `mkvmerge --quiet --output MERGED --no-global-tags --default-track-flag
   0:yes VIDEO_ONLY --no-video SOURCE`. Only after mkvmerge succeeds is that
   file renamed over the output.

Both temp files are reserved with `tempfile` (random part, exclusive
creation). So `x.mkv` and `x.mka`, or two concurrent jobs, never share one.
This changed on 2026-10-23; the names were fixed before then. A process that
is killed outright (SIGKILL) can leave them behind. The controller deletes
the whole `{task_id}/` directory, so this only matters for CLI runs.

What mkvmerge preserves:

- the source track order, including the order across track types;
- languages, default flags, chapters and global tags (the title);
- attachments.

mkvmerge also writes the BPS and NUMBER_OF_FRAMES statistics tags itself,
so mkvpropedit is not run on this path.

`MuxPlan::for_config` runs `mkvmerge --identify --identification-format
json SOURCE` before any frame is processed:

- If mkvmerge is missing, the job fails immediately, naming
  `setup_dev_media_tools.sh` and `bin_win64.zip`.
- If the JSON says `recognized` or `supported` is false (exit code 0), it
  logs a warning and falls back to the single FFmpeg pass.

How the mkvmerge step ends:

| mkvmerge exit | Result |
|---|---|
| 0 | Success; the merged file is renamed over the output and the video-only file is removed. |
| 1 | Warnings, still a success; same as 0. |
| 2 | Error quoting mkvmerge's stdout. The partial merged file is removed, an existing output is left untouched, and the encoded video is kept and named in the error. |

If the encoder is dropped before `finish`, its temp files are removed once
FFmpeg has exited. Non-Matroska outputs keep the old single FFmpeg pass.

`finish` only runs when the whole pipeline succeeded:

- `StreamingExecutor` joins every stage and drains the errors first.
- On cancellation or an upstream failure, the encoder is dropped, not
  finished. This was added on 2026-10-23. Before that, a cancelled MKV job
  muxed its truncated video with the full-length source audio.
- The check cannot happen inside the encoder stage. A failed stage closes
  its output channel before it reports the error, so the encoder sees a
  normal end of input.
- With the direct FFmpeg pass, the abandoned output is left unfinalized
  rather than deleted. FFmpeg truncates an existing output only when the
  first frame arrives, so before that the file may still belong to the user.

Verification:

- `mkv_output_keeps_copied_streams_interleaved_with_slow_video` is an
  ignored test that needs the real tools; run it with `PATH=$PWD/bin:$PATH`.
  It uses a source with a font, a one-line subtitle, 20 s and 1 s audio
  tracks, chapters and a title, plus a 3 s warm-up before any frame is
  written. With mkvmerge it passes (lead under 1 s). Forcing the old direct
  mux fails it with a 10 s lead.
- End-to-end CLI run on a real 2-minute clip with 14 subtitle tracks and
  14 fonts: lead 0.4 s.
  - Audio, subtitle and font payloads are byte-identical to the source.
  - Decoded video frames are identical to the old build. The first packet
    grows because mkvmerge repeats the HEVC/H.264 parameter sets in-band.
  - mkvmerge exits 0 on the real 4K 71.928 fps HEVC output.

Tooling:

- `bin_linux64.zip` on the misc release now contains a `bin/mkvmerge`
  wrapper, written the same way as the mkvpropedit one but with
  `ARGV0=mkvmerge`. The checksum is `be77fd66...`, and the previous upload
  is kept as `bin_linux64-2026-09-07.zip`.
- The Dockerfile pins the new checksum and keeps mkvmerge. Building only the
  `media-tools` stage locally confirms that the checksum matches and that
  mkvmerge/mkvpropedit 101.0 run in the stage image.
- `bin_win64.zip` was rebuilt by the `Windows media tools` workflow (run
  39121588173), using `scripts/media_tools.ps1 -Build`, and uploaded. Its
  SHA-256 is `0db8501b...`; the previous upload is kept as
  `bin_win64-2026-09-28.zip`.
  - mkvmerge.exe and mkvpropedit.exe now come from the official MKVToolNix
    101.0 portable zip (`mkvtoolnix-64-bit-101.0.zip`, SHA-256 `59a807b4...`
    per the publisher's `sha256sums.txt`). Both exes import only Windows
    system DLLs. The zip's `doc/COPYING.txt` ships as
    `MKVTOOLNIX-LICENSE.txt`.
  - `-Verify` now also muxes every encode case with source audio through
    mkvmerge and checks the NUMBER_OF_FRAMES tag that mkvmerge writes.
  - The FFmpeg pin moved to `autobuild-2026-09-30-13-08`
    (`ffmpeg-n8.1.3-9-g29e619e767-win64-gpl-8.1.zip`). The old pin,
    `autobuild-2026-09-26-13-03`, returned 404: BtbN deletes daily
    autobuilds after about two weeks and keeps only each month's last
    build, so always pin a month-end build.
  - The SHA256SUMS file inside the Windows zip uses CRLF line endings. To
    check it on Linux, run `tr -d '\r' < SHA256SUMS | sha256sum -c -`.
  - `-Verify` can be run on Linux with the official PowerShell container:
    `docker run --rm -v $PWD/scripts:/w/scripts:ro -v $PWD/bin:/w/bin:ro
    mcr.microsoft.com/powershell:7.4-ubuntu-22.04 pwsh -NoProfile -File
    /w/scripts/media_tools.ps1 -Verify -BinDir /w/bin`.
- Still open: remote workers keep the old bundle until
  `setup_dev_media_tools.sh` is re-run.
- In zsh, `ARGV0` is special and is not exported by `ARGV0=x cmd`. Use
  `env ARGV0=mkvmerge .../AppRun` when you test the AppRun by hand.

## Repairing an existing output

Remuxing with mkvmerge interleaves the file correctly. It takes about 5 s
for 2 GB on local disk and leaves the audio MD5 unchanged:

```bash
mkvmerge -o "out.fixed.mkv" "out.mkv"
```

A plain `ffmpeg -c copy` remux does not fix the file, because attachments
and sparse subtitles trigger the same scheduler read-ahead.

## Chapters / "skip intro"

That source has no chapters: `ffprobe -show_chapters` is empty, and there is
no Chapters element and no sidecar files. A remembered "skip OP" marker most
likely comes from the media server's intro detection (Jellyfin Intro
Skipper, Plex or Emby markers). That data is stored per file in the server
database, not in the MKV.
