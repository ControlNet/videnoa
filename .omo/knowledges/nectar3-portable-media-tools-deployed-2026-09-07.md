# Portable media tools deployed to nectar3 — 2026-09-07

User explicitly authorized downloading self-contained binaries and rsync deployment
into ~/videnoa, with no dependency-library installation. No apt/pip/system package
installation, videnoa restart, production media access or inference was performed.

## Artifacts and layout

Local download directory: /tmp/videnoa-portable-tools-11dg5ebr
FFmpeg source:
https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-09-06-13-06/ffmpeg-n8.1.2-50-g1a748fe2cd-linux64-gpl-8.1.tar.xz
Archive SHA256: 9b920f7f702cdc918654d37e517db82ae64a87619af1de15312d4ab9bf482383
Verified against that release's published checksums.sha256.
Includes x264/x265 and the h264_nvenc/hevc_nvenc encoders (enumeration only for NVENC).

MKVToolNix source:
https://mkvtoolnix.download/appimage/MKVToolNix_GUI-101.0-x86_64.AppImage
Downloaded file SHA256: b94c4e91146f46bb4a54e21ac29e8fcf58a60bcdf98944b1e923e73881f5eba7
This hash records the downloaded artifact; no separate publisher checksum verification
was claimed for the AppImage.

Rsync transferred tools into a fresh staging directory on nectar3:
/home/zhixi/videnoa/.media-tools-stage.xImYFe
SHA256 was checked before and after promotion. Previous ffmpeg, ffprobe and
mkvpropedit were preserved in:
/home/zhixi/videnoa/bin-backup.pQlKh7

Active tools:
- ~/videnoa/bin/ffmpeg and ffprobe: n8.1.2-50-g1a748fe2cd-20260906.
- ~/videnoa/bin/mkvpropedit: small shell launcher for version 101.0.
- ~/videnoa/bin/.mkvtoolnix-101.0/: official AppImage contents extracted once.
- ~/videnoa/bin/MKVToolNix_GUI-101.0-x86_64.AppImage: retained download.
- ~/videnoa/bin/media-tools-SHA256SUMS: transferred-artifact checksums.

The launcher resolves its directory, sets APPDIR to the private extracted tree
and ARGV0=mkvpropedit, then execs the publisher's AppRun with unchanged arguments.
This avoids FUSE/mount requirements and uses only bundled private libraries plus
already-present system libraries. It does not install libraries into system paths.
Preserve the hidden .mkvtoolnix-101.0 directory when relocating the tools.

## Verified on the actual Ubuntu 22.04 server

All three version commands succeeded. Ffprobe ldd resolved only already-present
system/runtime libraries, with no missing libav* dependency. Generated explicitly
synthetic one-second 64x64 black test clips using libx264 and libx265, probed codec
and dimensions, changed their titles with mkvpropedit, then read back titles with
ffprobe. Both H.264 and HEVC paths passed; no user media was modified. Test clips
remain in staging smoke subdirectories. Real inference, large media throughput
and NVENC execution remain for the user's testing.

Repeat basic checks from the server workspace:
```bash
cd ~/videnoa
./bin/ffmpeg -version
./bin/ffprobe -version
./bin/mkvpropedit --version
(cd bin && sha256sum -c media-tools-SHA256SUMS)
```
Expected: versions above, no missing-library errors, all checksums OK. New worker
subprocesses resolve these paths without requiring a service restart. Repository
packaging scripts and the public misc release assets have not yet been updated.
