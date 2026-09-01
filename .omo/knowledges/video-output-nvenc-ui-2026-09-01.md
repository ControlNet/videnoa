# VideoOutput NVENC UI contract

## Confirmed behavior

- `VideoOutput` supports `libx265`, `libx264`, `hevc_nvenc`, and `h264_nvenc`.
- Software codecs show `crf` and `x265_preset` controls.
- NVENC codecs show `cq_value` and `nvenc_preset` controls.
- Codec selection assigns a compatible default pixel format:
  - `libx265` -> `yuv420p10le`
  - `libx264` -> `yuv420p`
  - `hevc_nvenc` -> `p010le`
  - `h264_nvenc` -> `yuv420p`
- The backend input parser must propagate `cq_value`, `nvenc_preset`, and
  `x265_preset` into `EncoderConfig`; descriptor-only controls are insufficient.
- FFmpeg profiles are codec-aware: H.264 NVENC uses `high`; HEVC NVENC uses
  `main10` for 10-bit output and `main` for 8-bit output.
- Before starting an NVENC output process, the backend probes the selected
  encoder with FFmpeg. Only hardware-capability failures containing
  `unsupported device` or `No capable devices found` trigger a software
  fallback; unrelated FFmpeg configuration failures remain errors.
- The fallback preserves the requested codec family and bit depth:
  - `hevc_nvenc` -> `libx265` with `yuv420p10le`
  - `h264_nvenc` -> `libx264` with `yuv420p`
- NVIDIA A30 supports CUDA and TensorRT inference but has no NVENC engine. On
  that GPU, a workflow requesting `hevc_nvenc` logs
  `NVENC is unavailable; using software video encoder` and continues with
  `libx265` rather than failing later with a broken FFmpeg stdin pipe.

## Verification

```bash
cargo fmt --all -- --check
cargo test -p videnoa-core
cargo clippy -p videnoa-core --all-targets -- -D warnings
cargo build --workspace

cd web
npm test -- --run
npm run lint
npm run build
```

Manual browser QA used a real VideoOutput node and switched through all three
representative states: `libx265`, `hevc_nvenc`, and `h264_nvenc`. The rendered
controls and pixel formats matched the contract, and the browser console had no
errors.

Manual CLI QA used the reported FI 3x -> SR 2x workflow and source video on an
NVIDIA A30. The release binary selected `libx265`, advanced from the former
failure at frame 10 to frame 139, and produced a readable partial Matroska file.
`ffprobe` reported HEVC Main 10, `yuv420p10le`, and 3840x2160 video.
