# Linux media release asset replacement (2026-09-07)

Supersedes the release-publication pause in `mkvpropedit-statistics-diagnostic-2026-09-07.md`. The user subsequently reported successful super-resolution (900 frames) and interpolation (900 input frames, 899 pairs, 1799 output frames) jobs, including successful statistics tagging. The original isolated tagging failure remains unexplained; these successful runs do not establish its root cause.

## Published asset

- Release: https://github.com/ControlNet/videnoa/releases/tag/misc
- Asset: https://github.com/ControlNet/videnoa/releases/download/misc/bin_linux64.zip
- Size: 179713205 bytes.
- SHA256: `6d1944351a5d4d236eae8930dd6a68a4d7da73228e5c6c5657cbb3967cdeb1a9`.
- Replaced only this asset, as authorized by the user. Existing application release archives were not rebuilt.
- Downloaded the published asset again and verified byte-for-byte equality with the tested candidate using `cmp` and SHA256.
- Old 1607517-byte asset backup: `/home/zhixi/.local/share/videnoa/release-asset-backups/2026-09-07/bin_linux64.zip`, SHA256 `8afd7ac170380e7a42851c6dfaf9f5461b5fdfd3df2eebb66309cf4f8d6f0cd8`.

## Contents

ZIP root is `bin/`, compatible with `scripts/package_dist.sh` extraction. FFmpeg and ffprobe are the exact BtbN binaries deployed and tested on nectar3, version `n8.1.2-50-g1a748fe2cd-20260906`. mkvpropedit is the previously tested wrapper for the complete extracted official MKVToolNix 101.0 AppImage in `bin/.mkvtoolnix-101.0`. The wrapper and private directory must stay together. The original compressed AppImage is not redundantly included. Directory permissions were normalized to 0755; file permissions and the upstream relative symlink were preserved with `zip -r -y -6`.

The bundle includes upstream license/documentation files, provenance in `README-linux-tools.txt`, and a SHA256 manifest for 296 regular files. No system dependencies were installed. System glibc/basic runtime requirements remain; validation is on Ubuntu 22.04 x86_64, not all distributions.

Candidate, staging tree and downloaded verification copy: `/tmp/videnoa-linux-bin-release-20260907/`.

## Verification

`unzip -tq` passed. The candidate ZIP was transferred to nectar3 and extracted independently under `~/videnoa/.media-tools-stage.xImYFe/release-check-20260907`. All file checksums passed. Its ffmpeg, ffprobe and mkvpropedit version commands passed. Fresh synthetic 1-second 128x72 30-fps H.264/libx264 and HEVC/libx265 MKVs encoded successfully; statistics tagging succeeded and ffprobe reported both 30 decoded frames and `NUMBER_OF_FRAMES=30` for each. No user videos were modified.

Read-only re-verification of the retained test outputs:

```sh
ssh -i ~/.ssh/id_rsa_ansr zhixi@nectar3.neusym.cloud.edu.au 'bash -se' <<'REMOTE'
cd ~/videnoa/.media-tools-stage.xImYFe/release-check-20260907
(cd bin && sha256sum --quiet -c SHA256SUMS)
for codec in libx264 libx265; do
  bin/ffprobe -v error -count_frames -show_entries stream=codec_name,width,height,nb_read_frames:stream_tags=NUMBER_OF_FRAMES -of json "$codec.mkv"
done
REMOTE
```

Expected: no checksum errors; both files report 30 frames and a statistics frame-count tag of 30. No full application archive rebuild or new CI run was performed for this asset-only replacement. The separate stdout diagnostic source patch remains local and uncommitted.
