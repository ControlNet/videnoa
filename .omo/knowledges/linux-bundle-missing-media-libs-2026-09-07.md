# Confirmed missing Linux media dependencies — 2026-09-07

Read-only SSH inspection of the user's ~/videnoa deployment on an Ubuntu 22.04.5
x86_64 server confirmed this is not an iroh failure or a missing presets directory.
No server files, services, packages, credentials or configuration were modified.

Bundled ffprobe/ffmpeg require eight absent SONAMEs: libavdevice.so.58,
libavfilter.so.7, libavformat.so.58, libavcodec.so.58, libpostproc.so.55,
libswresample.so.3, libswscale.so.5 and libavutil.so.56. The lib directory contains
ONNX Runtime/CUDA/cuDNN/TensorRT libraries, not these FFmpeg libraries. Explicit
LD_LIBRARY_PATH pointing to the bundle lib does not fix ffprobe exit 127.
Readelf confirms these are DT_NEEDED entries; ffprobe has no RPATH/RUNPATH entry.
No ffmpeg or ffprobe executable was found on the remote shell's PATH.

Mkvpropedit also misses libmatroska.so.7, libebml.so.5, libpugixml.so.1,
libfmt.so.8, libQt5Core.so.5 and libdvdread.so.8.

The current public misc asset bin_linux64.zip was downloaded read-only and its
ZIP directory inspected in memory. It contains only bin/ffmpeg (301544 bytes),
bin/ffprobe (178832 bytes) and bin/mkvpropedit (4950736 bytes). SHA256 of all three
entries exactly matches the user's remote files. This rules out corrupted or
incorrectly copied tool executables and confirms that the bin source asset does
not carry their shared dependencies.

Current packagers extract bin/lib assets; cuDNN-presence and glibc/videnoa --help
checks do not validate the media-tool dependencies. The Linux bundle therefore
cannot currently be described as self-contained on an otherwise clean Ubuntu
22.04 host. A durable fix should package a coherent complete tool dependency set
with reliable child-process loader paths (or appropriately self-contained tool
builds) and execute ffprobe/ffmpeg/mkvpropedit in an isolated baseline image.
Simply setting LD_LIBRARY_PATH cannot compensate for missing files.

## Direct dependency inventory and portability

Remote readelf DT_NEEDED inventory:
- ffmpeg and ffprobe each: libavdevice.so.58, libavfilter.so.7,
  libavformat.so.58, libavcodec.so.58, libpostproc.so.55, libswresample.so.3,
  libswscale.so.5, libavutil.so.56, libm.so.6, libc.so.6.
- mkvpropedit: libmatroska.so.7, libebml.so.5, libz.so.1, libpugixml.so.1,
  libfmt.so.8, libQt5Core.so.5, libgmp.so.10, libstdc++.so.6, libdvdread.so.8,
  libm.so.6, libgcc_s.so.1, libc.so.6.
All three executables request GLIBC_2.34 as their highest directly referenced
GLIBC version. This is a lower bound, not the final bundle glibc floor: transitive
libraries may require newer symbols. Existing project packaging targets <=2.35.

Ubuntu package metadata confirms further codec, graphics, audio and C++ library
dependencies: libavcodec58 pulls x264/x265/vpx/aom/dav1d and others; libavdevice58
pulls ALSA/PulseAudio/SDL/X11/GL-related packages; QtCore pulls ICU70/pcre2/zstd etc.
Apt package recursion includes alternatives/data packages and is not an exact ELF
shared-object closure. The complete packaging manifest must come from actual
resolved libraries in one controlled build environment, recursively inspecting
DT_NEEDED and any required runtime-loaded modules. Do not call the direct list
above a complete portable bundle manifest.

Cross-distribution portability requires matching CPU architecture, ABI/SONAMEs,
symbol versions and the full dependency closure. Carry a coherent non-system
library set from a supported baseline, preserve versioned symlinks and configure
child-process-local loader paths or per-object ORIGIN-relative RUNPATH. RUNPATH
on an executable alone does not apply recursively to its dependency children.
Avoid globally overriding the worker's inference libraries and do not casually
bundle libc/libm/ld-linux as ordinary optional libraries. A glibc/loader pairing
needs separate design if supporting a different libc baseline.

References:
https://man7.org/linux/man-pages/man8/ld.so.8.html
https://gcc.gnu.org/onlinedocs/libstdc++/manual/abi.html
No system packages were installed and no live server files were changed.
