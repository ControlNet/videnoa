# ort rc.12 api-23 Migration - 2026-08-24

## Decision

- Use `ort = "=2.0.0-rc.12"` with `default-features = false`, `load-dynamic`, and explicit `api-23`.
- Retain the existing ONNX Runtime 1.23.2 runtime and provider libraries under `lib/`.
- Do not mix ONNX Runtime core/provider library versions or enable `api-24` while loading 1.23.2.

## Required source migration

- Import execution providers from `ort::ep` instead of the deprecated `ort::execution_providers` compatibility module.
- Import `TensorElementType` from `ort::value` instead of the removed `ort::tensor` module.
- rc.12 session-builder mutators return `ort::Error<SessionBuilder>` with a recovery value. Before propagating into `anyhow`, convert it to `ort::Error<()>` with:

```rust
.map_err(|error| -> ort::Error { error.into() })?
```

- `SessionBuilder::commit_from_file` requires a mutable builder in the standalone TensorRT probe closure.

## Verification

- Initial `cargo check --workspace --all-targets` failed for the expected removed tensor path and non-`Send`/`Sync` builder recovery error conversion.
- Final `cargo check --workspace --all-targets` passed.
- `cargo test --workspace`: 555 passed, 9 ignored, 0 failed.
- `cargo build --release --workspace` passed.
- CUDA spike: CUDA EP available; output shape `[1, 3, 2880, 5120]`.
- IoBinding spike on `libonnxruntime.so.1.23.2`: CUDA-pinned output extraction succeeded with shape `[1, 3, 640, 960]`, finite values, and normal process teardown.
- TensorRT CLI reused the warmed four-file cache.
- Fixed 120-frame output: HEVC, 3840x2160, `yuv420p10le`, 120 frames, 5.005 seconds.
- Decoded framemd5 SHA256 matched the established baseline:

```text
6b68085e0dde975988bb7e978c4d2ea461d8b2df4ec7ab5ec2a6d33c49bedb2b
```

- Oracle migration review approved unconditionally.

## Notes

- `Cargo.lock` is intentionally ignored by this repository; the exact manifest pin prevents resolving a later `ort` release.
- The IoBinding harness must inspect the explicit `CUDA_PINNED output extraction OK` line because other optional IoBinding branches print failures rather than returning them.
