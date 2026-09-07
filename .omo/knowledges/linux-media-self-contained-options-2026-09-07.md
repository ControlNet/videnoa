# Prefer self-contained Linux media tools — 2026-09-07

The user prefers a simpler portable package over collecting distribution .so files.
The dependency-copying approach previously discussed is not the only option.

FFmpeg's official download page links to BtbN static builds. BtbN documents
linux64 builds with glibc >=2.28 and Linux >=4.18. Its gpl variant includes x264
and x265, which Videnoa exposes/uses; lgpl omits those encoders. Choose non-shared
builds when evaluating portable ffmpeg/ffprobe binaries. Static media libraries
remove the external libav* dependency problem but do not mean no OS/GPU-driver
requirements. Verify actual encoders, filters, protocol behavior and CLI outputs
before replacing the pinned distribution assets.

MKVToolNix officially distributes an AppImage for glibc >=2.28. Its documented
CLI dispatch supports a symlink named mkvpropedit to the AppImage. This bundles
libraries into one artifact rather than statically linking everything. Headless
server/AppImage runtime requirements still need validation; it must not be claimed
to work on the remote deployment until executed there. Consider extracted AppDir
packaging if avoiding FUSE is necessary, but this was not implemented.

Sources checked:
https://ffmpeg.org/download.html
https://github.com/BtbN/FFmpeg-Builds
https://mkvtoolnix.download/downloads.html
No existing remote binaries, dependency archives or packaging scripts changed.
