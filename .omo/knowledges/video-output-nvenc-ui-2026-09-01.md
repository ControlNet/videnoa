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
- Before starting an NVENC output process, the backend probes the exact selected
  encoder with FFmpeg. Any probe failure stops encoder creation and preserves
  the original FFmpeg diagnostic.
- Codec selection is authoritative. The backend must never implicitly replace
  `hevc_nvenc` with `libx265` or `h264_nvenc` with `libx264`. Software encoding
  is used only when the user explicitly selects a software codec.
- NVIDIA A30 supports CUDA and TensorRT inference but has no NVENC engine. On
  that GPU, a workflow requesting `hevc_nvenc` fails before frame processing
  with `unsupported device` and `No capable devices found`, clearly identifying
  the requested codec and stating that software fallback was not applied.

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
NVIDIA A30. The command exited with status 1 before frame processing, and the
error preserved the requested `hevc_nvenc` codec, `unsupported device`, and
`No capable devices found`. No software encoder process or output file was
created.
