# Current Preview State (2026-08-23)

- The preview UI exists as a global modal (`ComparisonViewer`) opened from the editor toolbar's rightmost eye icon. The icon has no visible text; its Preview label is tooltip-only.
- `/api/preview/extract` is functional and uses FFprobe plus FFmpeg to extract 10 evenly spaced PNG frames into a temporary preview directory.
- Frame extraction first runs `ffprobe -count_frames`, which decodes/scans the full video. On the repository's 23:43 `1.mkv`, the UI remained in `Extracting frames...` for more than one minute. This makes preview appear absent on long videos.
- `/api/preview/process` is a placeholder. It accepts the current workflow but does not read or execute it; it returns the selected original frame URL unchanged.
- Therefore the current feature is an extraction/comparison UI prototype, not a real model preview. "Before" and "After" are identical after processing.
- Preview sessions are stored in memory with temporary directory paths; no TTL or explicit cleanup path was found.
- Runtime UI verification used the existing release server at `127.0.0.1:3000`, opened the eye icon with Playwright, entered `1.mkv`, and observed the long-running extraction state. The temporary server and Playwright artifacts were removed afterward.

## CUDA Runtime Verification

- The RealESRGAN CUDA smoke test passed with an 8x8 RGB input and produced the expected 32x32 output in 3.59 seconds.
- The tiled CUDA inference test passed with a 64x64 RGB input and produced the expected 256x256 output in 1.52 seconds; eight consecutive repetitions also completed successfully.
- Running the same RealESRGAN test with `CUDA_VISIBLE_DEVICES=-1` failed during CUDA execution-provider initialization with `CUDA failure 100: no CUDA-capable device is detected`. Together with the successful visible-device runs and the provider's `error_on_failure()` configuration, this establishes that the tested path requires and initializes the NVIDIA GPU rather than silently using a CPU-only session.
- External `nvidia-smi` sampling did not capture the short-lived test process or a VRAM peak, so exact utilization and peak-memory figures remain unmeasured.

## Long-Video GPU Utilization Investigation

- The representative workload used `1.mkv`: HEVC, 1920x1080, 24000/1001 fps, and 1422.942 seconds. The workflow was `presets/anime-2x-upscale.json`, using AnimeJaNai V3 L1 Sharp HD x2 FP16 through TensorRT with `tile_size: 0`, producing 4K frames.
- On the NVIDIA A30-24C, the default `libx265` path sustained about 10.5 fps. Across roughly 1005 samples at 200 ms intervals, GPU utilization averaged 17.14%, had an 18% median and 18% P95, and peaked at 19%. GPU memory utilization averaged 4.85%; allocated memory peaked at 1881 MiB versus a 921 MiB idle baseline. NVDEC and NVENC remained at 0%.
- Replacing video encoding with a null sink sustained about 10.8 fps. GPU utilization averaged 18.21%, with an 18% median and 19% P95/max; memory utilization averaged 5.06%, and allocated memory still peaked at 1881 MiB.
- Removing software encoding therefore improved throughput by only about 0.3 fps and compute utilization by about one percentage point. This experimentally rules out `libx265` backpressure as the primary cause of the low GPU utilization.
- The observed utilization remained steadily around 17-18% rather than showing isolated drops, while the SM clock held at 1440 MHz. The workload is consistently under-filling the GPU rather than intermittently losing the device.
- The main limiting path is single-frame synchronous execution: one inference call at a time, one ONNX Runtime session, no batching, and CPU-side RGB-to-FP16-NCHW preprocessing plus output allocation/conversion around each call. I/O binding is enabled, but the surrounding per-frame CPU work, host/device transfers, and synchronization prevent enough concurrent GPU work from accumulating.
- The streaming executor's bounded channels permit stages to overlap, but each normal stage has one worker and the inference stage itself remains synchronous. Increasing channel depth alone will not create concurrent model executions.
- Optimization priority is: add multi-frame batching or controlled concurrent inference; keep tensors GPU-resident across compatible stages and reduce per-frame allocations/transfers; optimize or move color/layout conversions to the GPU; then evaluate hardware decoding. Hardware encoding is a lower-priority optimization for this workload because the null-sink comparison showed little benefit.
- A separate encoder configuration defect was identified: `VideoOutputNode::execute()` returns only `output_path`, while `VideoCompileContext::create_encoder()` reads `codec`, `crf`, and `pixel_format` from the node output map. Workflow-level encoder settings are consequently discarded and the defaults (`libx265`, CRF 18, `yuv420p10le`) are used.
- A forced `hevc_nvenc` comparison could not run on this A30 environment: FFmpeg failed at `OpenEncodeSessionEx` with `unsupported device`. This prevents a local NVENC throughput measurement but does not affect the null-sink evidence that software encoding is not the dominant limiter.
- The A30 telemetry interface did not expose useful power or PCIe metrics, and the measurements were whole-device rather than per-process. Two otherwise idle Python CUDA contexts occupied about 920 MiB combined, but baseline compute utilization was 0%, so they do not explain the sustained utilization observed during Videnoa processing.
