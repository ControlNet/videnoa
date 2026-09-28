# Review fixes 2–5, 2026-09-26

Scope: fixes the second through fifth findings in
`project-review-2026-09-26.md`, starting from `46c72e1`. The user explicitly
deferred finding 1; Downloader filename and directory behavior is unchanged.

## Cancellation

`ExecutionContext` now carries the existing watch receiver. Scalar execution
checks cancellation before and after node execution, nested workflows inherit
it, and video compilation checks it between initialization steps. The Worker
uses one execution path for workflows with and without submitted parameters.
An already-cancelled token initializes its watch as cancelled synchronously.

The regression uses an actual local HTTP service: hold the first node's request,
DELETE the job, release the first response, then verify the downstream HTTP
request never arrives and the next job can acquire admission and complete. It
covers both a direct scalar graph and a nested Workflow node.

This is cooperative cancellation between execution steps. It does not interrupt
an already-running synchronous HTTP request, download or model initialization;
that operation must return before the executor observes cancellation. Deleting
a job still removes its API record as before.

## Declared input types

The Worker injects JSON into WorkflowInput node parameters for both scalar and
VideoFrames graphs, reusing the existing decoder for the declared port type.
JSON strings declared as Path therefore reach Path consumers as PortData::Path.
Regression coverage includes inferred parameters, explicit parameters without a
node-local fallback, and explicit empty parameters with node-local values.

## Real single-frame preview

The processing endpoint validates the supplied graph and executes existing
SuperResolution, Resize and Rescale processors on the selected extracted image.
It writes a unique PNG and returns that URL. Input images and the workflow's
formal video output are preserved. Preview shares the Worker's GPU semaphore;
the blocking task owns the permit even if its HTTP caller disconnects.

Supported graphs have one VideoInput, one VideoOutput and a linear frame path.
Supported scalar helpers are Constant, WorkflowInput, WorkflowOutput,
PathDivider, PathJoiner, StringTemplate, StringReplace and TypeConversion.
FrameInterpolation and SceneDetect return a clear multiple-frame requirement;
other unsupported nodes, including HTTP/download/nested workflow actions, are
rejected before execution. Preview does not evaluate final video codecs or
compression quality. TensorRT initialization uses the existing cache identity
and can incur normal first-build cost; TensorRT was not exercised in this fix.

Media integration tests generate a synthetic 32x32 video with FFmpeg. Resize
produces a distinct 16x8 PNG, repeated processing returns distinct URLs, and the
original image remains byte-identical. Invalid/temporal/side-effect graphs and
overflowing frame indices are rejected. The ignored GPU test loads the actual
`anime-4x-upscale.json` preset, changes its backend to CUDA and disables tiling
for the tiny fixture, and verifies an actual 128x128 PNG from RealESRGAN. It was
explicitly run and passed on the local NVIDIA GPU. Media tests require FFmpeg;
the normal media tests print a skip message if it is unavailable.

The viewer invalidates outstanding request ownership on frame selection, new
extraction, modal lifetime changes and unmount. Late results cannot populate a
different frame; a failed reprocess does not retain an older result as success.
Frontend tests use explicitly synthetic API responses for these timing cases.

## Controller detail recovery

`useTaskDetail` subscribes to `appInvalidationStore` and uses its existing reload
path on a new generation. Initial, reconnect and lag invalidations refresh the
selected detail without clearing visible content or expanded attempt history.
The existing task-delta refresh and request ownership behavior is retained.
No Controller backend scheduling, transfer or event protocol changes were made.

## Verification commands

Run from the repository root. Every quality gate should exit zero. The GPU
command requires the documented conda `anime` runtime libraries and local model.

```bash
export ORT_DYLIB_PATH=$PWD/lib/libonnxruntime.so
export TRT_LIBS=$HOME/miniconda3/envs/anime/lib/python3.13/site-packages/tensorrt_libs
export LD_LIBRARY_PATH=$TRT_LIBS:$PWD/lib:$HOME/miniconda3/envs/anime/lib:$LD_LIBRARY_PATH
export PKG_CONFIG_PATH=$HOME/miniconda3/envs/anime/lib/pkgconfig:$PKG_CONFIG_PATH

cargo fmt --all -- --check
cargo test --locked -p videnoa-core -p videnoa-app -p videnoa-transport --lib --tests
cargo test --locked -p videnoa-core --test server_regressions preview_superresolution_cuda_produces_upscaled_png -- --ignored --nocapture
cargo clippy --locked -p videnoa-core -p videnoa-app --all-targets -- -D warnings
cargo test --locked -p videnoa-controller --all-targets
npm --prefix web test
npm --prefix web run lint
npm --prefix web run build
npm --prefix controller-web test
npm --prefix controller-web run lint
npm --prefix controller-web run build
```

Worker/Core/App/Transport: 661 tests passed, 12 ignored in the regular run.
The GPU preview test was separately run with `--ignored` and passed. The
allocator-policy integration executable also passed its three subprocess cases.
Formatting and strict Core/App Clippy passed.

Controller validation covered all 600 passing tests, with 1 ignored, across the
all-target run and targeted follow-ups. It was not a clean single invocation:
the first run failed the log-capture assertion
`committed_creation_is_logged_once_without_private_request_data`; that test
passed in the next all-target run. The next run passed 507 tests, then all 44
`task_api` tests timed out opening their SQLite fixture pools before business
assertions. Rerunning that target with four test threads passed all 44 in 2.09
seconds. The seven remaining targets then passed another 49 tests with four
threads. No Controller backend source was changed to obtain these results.
The 37 task20 fault/restart tests passed in 218.43 seconds.

Exact targeted follow-up commands:

```bash
cargo test --locked -p videnoa-controller --test task_api -- --test-threads=4
cargo test --locked -p videnoa-controller --test task_api_concurrency --test task_contract --test value_contract --test videnoa_client --test windows_path_contract --test worker_password --test workspace_paths -- --test-threads=4
```

Worker web: 197 tests passed; Controller web: 182 tests passed. Both lint and
production builds passed. Existing build warnings remain for the Worker bundle
size and third-party Zod PURE comments in the Controller bundle. GPU execution
emitted conda ncurses/tinfo version-information warnings, but processing passed.
No deployment, release packaging or physical sleep/resume test was performed.
