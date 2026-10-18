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

## Cause

`EncoderConfig::build_ffmpeg_args` (`crates/core/src/nodes/video_output.rs`)
runs one ffmpeg process with two inputs:

- input 0 is the processed frames on `pipe:0`, produced at AI speed;
- input 1 is the source file, from which audio, subtitles and attachments
  are copied with `-c copy`.

The real trigger is the FFmpeg 7+ CLI scheduler (`fftools/ffmpeg_sched.c`,
`trailing_dts` / `schedule_update_locked`), not just the slow pipe:

- The scheduler finds the output stream that is furthest behind (the
  trailing DTS). It lets only the inputs feeding streams within
  `SCHEDULE_TOLERANCE` (100 ms) of that stream keep reading, and pauses the
  others.
- If any unfinished output stream has no DTS yet, the trailing DTS is
  unknown, and only the sources of streams without a DTS may read. Font
  attachments never produce packets, so their stream has no DTS until its
  demuxer reaches EOF. Because the attachments shared input 1 with the
  audio, the source demuxer read the whole file at once, and the muxer's
  `max_interleave_delta` (10 s) flushed all of that audio before any video.
  With a 2-minute real clip, audio landed 110 s ahead (the clip minus 10 s).
- Sparse subtitle tracks cause the same problem on a smaller scale. During a
  gap in the dialogue the subtitle stream is the trailing stream, so the
  shared demuxer reads ahead to the next line, and the audio it reads
  arrives early too. Without fonts but with sparse subtitles, the lead was
  still about 17 s, and it gets longer during a silent OP.
- These measurements are the same whether the video comes from the pipe or
  from a separate video-only file. So a two-stage ffmpeg remux does not
  help, and neither does `-max_interleave_delta 0`: that option buffered
  the whole output in RAM when a subtitle track ended early (the file
  stayed at 0 bytes during the encode).

## Repairing an existing output

Remuxing with mkvmerge interleaves the file correctly. It takes about 5 s
for 2 GB on local disk and leaves the audio MD5 unchanged:

```bash
mkvmerge -o "out.fixed.mkv" "out.mkv"
```

A plain `ffmpeg -c copy` remux does not fix the file. Attachments and
sparse subtitles cause the same scheduler read-ahead described above.

## Fix (implemented 2026-10-18)

`build_ffmpeg_args` gives each kind of copied stream its own demuxer of the
source:

```
-i pipe:0 -i SRC -t 1 -i SRC -i SRC
-map 0:v:0 -map 1 -map -1:v -map -1:s -map -1:t -map 3:s? -map 2:t?
```

- Input 1 carries audio (and data streams), input 3 carries subtitles, and
  input 2 carries attachments.
- Fonts are in the header, so `-t 1` lets the attachment input reach EOF
  almost immediately, which finishes its stream.
- When a subtitle gap makes input 3 read ahead, it discards the audio and
  video it reads, so nothing reaches the muxer early. The other inputs
  simply wait.
- Unused inputs (sources without subtitles or attachments) are probed but
  not read.
- Cost: the source is read twice, once by input 1 and once by input 3.

Verified with an end-to-end CLI run: VideoInput to VideoOutput, libx265,
on a 2-minute clip with 14 subtitle tracks and 14 fonts.

| Build | Audio written ahead of all video |
|---|---|
| old binary | 110 s |
| new binary | 0 s |

Stream order, language, default flags and BPS tags match the source. The
audio, subtitle and font payloads are byte-identical.

## Chapters / "skip intro"

That source has no chapters: `ffprobe -show_chapters` is empty, and there is
no Chapters element and no sidecar files. VideoOutput already passes
`-map_chapters 1`. A remembered "skip OP" marker most likely comes from the
media server's intro detection (Jellyfin Intro Skipper, Plex or Emby markers).
That data is stored per file in the server database, not in the MKV.
