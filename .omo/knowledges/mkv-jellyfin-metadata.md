# MKV and Jellyfin metadata preservation

Observed with FFmpeg 4.4 and mkvtoolnix 65:

- `-map_metadata 1` preserves ordinary global and per-stream tags, but it does not preserve every Matroska track-header property.
- Matroska's default `default_mode=infer` rewrites multiple source `Default` flags so only the first track of each type remains default. Use `-default_mode passthrough` and explicitly set `-disposition:v:0 default` because generated rawvideo has no source disposition.
- Raw RGB input has no sample aspect ratio. Add `setsar=1` to the output filter chain so Jellyfin reports `1:1` SAR and `16:9` DAR.
- Keep NTSC-derived rates as exact rationals before multiplying for FI. For example, 23.976 fps should remain `24000/1001`, producing `72000/1001` for 3x FI.
- FFmpeg's Matroska muxer stores `DefaultDuration` as integer nanoseconds. Therefore `72000/1001` can be probed as an equivalent nearby rational such as `21003/292`; x265 still carries the exact `72000/1001` timing internally.
- Matroska segment UID, track UIDs, writing application, statistics-writing application, video codec profile, bit depth, resolution, frame count, and bitrate are expected to change when the video is regenerated.
