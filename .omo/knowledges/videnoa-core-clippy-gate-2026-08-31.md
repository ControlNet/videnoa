# videnoa-core Clippy Gate

## Verified command

Run Clippy with every core target enabled so test-only warnings are included:

```bash
export ORT_DYLIB_PATH="$PWD/lib/libonnxruntime.so"
export TRT_LIBS="$HOME/miniconda3/envs/anime/lib/python3.13/site-packages/tensorrt_libs"
export LD_LIBRARY_PATH="$TRT_LIBS:$PWD/lib:$HOME/miniconda3/envs/anime/lib:$LD_LIBRARY_PATH"
export PKG_CONFIG_PATH="$HOME/miniconda3/envs/anime/lib/pkgconfig:$PKG_CONFIG_PATH"
cargo clippy -p videnoa-core --all-targets -- -D warnings
```

## Durable findings

- `--all-targets` is required: the 2026-08-31 baseline contained 19 warnings that appeared only in test targets.
- Shared callback signatures should use named type aliases. This keeps Clippy type-complexity fixes consistent across compile, executor, and server boundaries.
- Pipeline input/output frame totals belong in `PipelineFrameCounts`; all production and nested test callers must construct the same typed value.
- Inference dimensions and model input/output names belong in `InferenceShape` and `ModelBindings`, respectively. These types reduce positional-argument mistakes in single and tiled inference paths.
- After inference-path refactors, check for superseded conversion helpers. The old `run_fp16_inference` path became unreachable once direct FP16 and micro-stage inference owned that behavior.

## Verification result

On 2026-08-31, formatting and the exact Clippy command passed, `cargo test -p videnoa-core` passed 564 tests with 10 intentionally ignored, and `cargo build --workspace` completed successfully.
