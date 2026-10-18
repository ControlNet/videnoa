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

The source demuxer reads at disk speed while video packets trickle in, or
have not been emitted at all during model warm-up. Once the DTS gap is
larger than libavformat's default `max_interleave_delta` (10 s), the muxer
flushes the queued audio and subtitle packets without waiting for video.
So the whole audio track lands before the first video frame. A longer
episode or a high-bitrate lossless track could also go over ffmpeg's
pre-mux queue limit (`muxing_queue_data_threshold`, 50 MB) and fail with
"Too many packets buffered".

## Repairing an existing output

Remuxing with mkvmerge interleaves the file correctly. It takes about 5 s
for 2 GB on local disk and leaves the audio MD5 unchanged:

```bash
mkvmerge -o "out.fixed.mkv" "out.mkv"
```

A single-input `ffmpeg -i out.mkv -map 0 -c copy` is not a reliable fix: one
demuxer reads the file in its current order, so the muxer meets the same
gap.

## Code fix options (not implemented yet)

1. Add `-max_interleave_delta 0` to the output args. libavformat then
   buffers until every stream has a packet. Costs:
   - RAM grows to the size of every copied stream that runs ahead of the
     video (fine for ~35 MB of AAC, GBs for lossless movie audio);
   - a sparse or empty subtitle track makes video packets pile up in RAM
     until that track's next packet, or until the trailer.
2. Two stages: encode video only into a temp file, then remux:
   `ffmpeg -i temp -i source -map 0:v:0 -map 1 -map -1:v -c copy
   -map_metadata 1 -map_chapters 1 ...`. With two file inputs, the ffmpeg 7+
   scheduler throttles each demuxer, so packets interleave correctly. Costs
   temp disk about the size of the output, plus a few seconds. Do not
   require mkvmerge, because the bundled `bin/` only ships mkvpropedit.
