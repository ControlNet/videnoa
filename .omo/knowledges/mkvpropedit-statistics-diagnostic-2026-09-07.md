# MKV statistics failure investigation (2026-09-07)

This supplements the portable media tools deployment record. Release asset replacement is paused pending diagnosis of a reported statistics-tag failure.

- The interpolation job reported `mkvpropedit --add-track-statistics-tags` exit code 2 with empty stderr; a subsequent super-resolution job completed the same operation successfully.
- `VideoEncoder::finish` waits for successful FFmpeg exit before adding tags. Tagging failures are warnings and do not fail the job. Job completion alone therefore does not establish that statistics tags were added or that the entire output is valid.
- The application discarded mkvpropedit stdout. The diagnostic patch captures and logs stdout together with stderr on failure; this does not change tagging or job success behavior. The patch has not been deployed to the remote worker.
- On nectar3, the deployed MKVToolNix 101.0 wrapper successfully added statistics tags to copies of both existing synthetic H.264 and HEVC smoke-test files. These tests exercise the actual tagging command but do not reproduce the user's failing video.
- The two reported `data/workspace/.../output.mkv` paths no longer existed when inspected. The failing downloaded output is needed for reproduction on a copy. The exact cause is not yet known; neither a general binary failure nor output corruption has been established.
- Diagnostic copies are under `~/videnoa/.media-tools-stage.xImYFe/statistics-check.*`. No system dependencies were installed and no user video was modified.
