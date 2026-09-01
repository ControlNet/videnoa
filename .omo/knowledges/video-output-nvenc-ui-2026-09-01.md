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
