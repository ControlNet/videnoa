# Docker GPU verification on nectar3

Verified on 2026-10-06 against the running `videnoa` container using image
`controlnet/videnoa:latest`.

- Host and container both expose an NVIDIA A40-24Q with 24576 MiB VRAM and
  driver 580.65.06.
- Docker `HostConfig.DeviceRequests` requests all GPUs (`Count: -1`,
  `Capabilities: [["gpu"]]`). The reported `runc` runtime does not prevent
  GPU access in this deployment.
- A real CLI workflow inside the existing container successfully ran
  `RealESRGAN_x4plus_anime_6B.onnx` with the CUDA execution provider.
- The input was synthetic FFmpeg `testsrc2` test footage: 24 frames at 12 fps,
  320x180. Output was verified with ffprobe: H.264, 1280x720, 24 frames.
- Logs reported `Building session with CUDA EP` and
  `Workflow completed successfully`; process exit code was zero.
- GPU sampling observed memory increasing from zero to 2346 MiB and nonzero
  utilization (including 19%). Processing took approximately 10 seconds;
  this small synthetic smoke test is not a production performance benchmark.
- Encoding used CPU `libx264`. TensorRT, NVENC, frame interpolation, and
  controller-to-worker scheduling were not tested.

## Isolation and cleanup

All test files were created under a unique container
`/tmp/videnoa-gpu-check.*` directory. The working directory, `TMPDIR`, `HOME`,
`XDG_CACHE_HOME`, and `VIDENOA_DATA_DIR` were scoped to that directory.
An EXIT trap stopped the GPU sampler and removed only that test directory.
Both the initial argument-validation attempt and successful inference attempt
confirmed cleanup. No worker configuration or existing models were changed.

For CLI `run`, use `VIDENOA_DATA_DIR` to isolate runtime logs/data:
`videnoa --data-dir ... run ...` is rejected by this version's argument parser.
Use absolute model paths when running from an isolated working directory.
Symlink the bundled media tools into the temporary working directory's `bin/`
to retain bundled FFmpeg discovery.

## Read-only verification commands

Run these on the worker host:

```bash
docker inspect --format 'DeviceRequests={{json .HostConfig.DeviceRequests}} Runtime={{.HostConfig.Runtime}}' videnoa
docker exec videnoa nvidia-smi
docker ps --filter name=videnoa --format 'table {{.Names}}\t{{.Status}}'
docker exec videnoa find /tmp -maxdepth 1 -name 'videnoa-gpu-check.*' -print
```

Expected: GPU requests are present, container sees the A40, worker is healthy,
and the final command prints no test directories. Idle zero GPU utilization
does not imply that GPU access is broken; observe it during inference.
