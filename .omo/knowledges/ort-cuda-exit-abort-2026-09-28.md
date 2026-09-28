# Intermittent abort at process exit after CUDA inference — 2026-09-28

## Symptom

`videnoa run` prints "Workflow completed successfully". It then sometimes
aborts with glibc `corrupted double-linked list` (exit 134, SIGABRT). The
output file is complete. The abort is not tied to one model: AnimeJaNai fp16
and ESRGAN fp32, tiled and untiled, all hit it.

| Case | Abort rate before the fix |
| --- | --- |
| `D-janai-2x` workflow | 13 of 20 runs |
| Same workflow with no CUDA stage | 0 of 10 runs |

## Root cause

- The ort crate (2.0.0-rc.12, `src/environment.rs`) releases its global
  `G_ENV` from a function in the executable's `.fini_array`.
- glibc `exit()` first runs the `atexit`/`__cxa_atexit` handlers. These
  include the C++ static destructors of the dlopen'd `libonnxruntime.so` and
  `libonnxruntime_providers_cuda.so`. Only after that does `_dl_fini` run the
  `.fini_array` entries.
- So `ReleaseEnv` runs on already-destroyed ORT state and corrupts the heap.
- glibc only notices later, when libcuda's own destructor calls `free()`. That
  is the last fini in the `LD_DEBUG=files` trace and the frame in the gdb
  backtrace.

## Evidence

- gdb (installed in a throwaway conda prefix in the scratchpad) showed the
  abort on the main thread: `free()` called from libcuda.so.1 inside ld.so's
  fini. Other threads were idle.
- Ruled out, each by an A/B run:
  - delaying exit;
  - IoBinding and the CUDA-pinned output allocator;
  - `mallopt(M_ARENA_MAX)`;
  - mixing CUDA libs from `lib/` and conda (conda-only 12.8 / cuDNN 9.14
    still aborted);
  - the streaming executor. A bare ort CUDA session with a plain
    `session.run` still aborted about 10% of the time; the CPU EP never did.
- Keeping an extra `Arc<Environment>` alive, so that `ReleaseEnv` is never
  called: 0 of 20 CLI runs aborted, against 13 of 20 without it.

## Fix

- `nodes/backend.rs::retain_ort_environment()` stores
  `ort::environment::current()` in a static `OnceLock`, which is never
  dropped.
- It is called from `build_session`, the only production session
  constructor.
- The environment now lives until the process exits, and the OS reclaims it.

## Regression test

- `nodes::super_res::tests::process_exits_cleanly_after_cuda_inference` is an
  ignored GPU test.
- It re-runs the test binary as a child 10 times. The child is
  `cuda_inference_then_exit_child`: 640x360 fp16 micro-stage inference, with
  inference on another thread.
- Each child must exit 0.
- Before the fix it failed within the first 6 children in 3 of 3 runs. After
  the fix it passed 5 of 5 runs, 50 children in total.
- Small frames (96x80) did not reproduce the abort.

## Notes

- Revisit this workaround if ort is upgraded; upstream may fix the ordering.
- Useful commands:
  - `LD_DEBUG=files LD_DEBUG_OUTPUT=<prefix>` prints the fini order per
    process.
  - `malloc_trim(0)` succeeding does not prove the heap is intact.
