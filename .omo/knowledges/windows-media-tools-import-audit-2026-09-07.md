# Windows media-tool import audit — 2026-09-07

Downloaded the current public misc/bin_win64.zip (128467312 bytes) into a temporary
local audit directory and inspected executable PE import tables with objdump -p.
No Windows executable was run. Archive contains only:
- bin/ffmpeg.exe: 169963520 bytes;
- bin/ffprobe.exe: 169757184 bytes;
- bin/mkvpropedit.exe: 16216104 bytes.

Ffmpeg and ffprobe import Windows platform DLLs and api-ms-win-crt-* UCRT APIs;
neither imports separate avcodec/avformat/avdevice/etc. DLLs. Mkvpropedit imports
Windows platform DLLs/msvcrt and does not import separate Qt/Matroska/EBML DLLs.
Thus the ordinary import tables do not show the Linux bundle's missing third-party
shared-library problem. This is evidence of a more self-contained Windows tool
build, not proof of arbitrary Windows version compatibility, dynamically loaded
optional libraries, GPU execution, or real media processing.

Windows packaging CI currently validates bundle entries and archive integrity;
it does not explicitly execute the bundled media tools. Passing Windows Rust
and loopback iroh tests is separate evidence and does not validate these downloaded
media executables. Add actual ffmpeg/ffprobe/mkvpropedit version checks and a short
real media probe/encode in a Windows packaging test before claiming runtime
acceptance for the media bundle.
