# nectar3: videnoa container lost the GPU while running (2026-10-08)

Access: `ssh -i ~/.ssh/id_rsa_ansr nectar3.neusym.cloud.edu.au` (host
`ansr-nectar-3`, user `zhixi`, groups `sudo docker`, TZ Etc/UTC). The
user has no journal access without sudo.

## Symptom

From 2026-10-08 15:25 UTC every job failed within 0.1 s:

```
CUDA failure 100: no CUDA-capable device is detected ... cudaGetDeviceCount
TensorRT session initialization failed
Job execution failed error=failed to initialize FrameInterpolation node
  Caused by: Failed to load ONNX model: models/rife_v4.26.onnx
```

The container stayed `Up (healthy)`, 0 restarts, started 2026-10-05
10:52 UTC. A CUDA CLI smoke test inside the same container passed on
2026-10-06.

## Evidence (read-only)

- Host `nvidia-smi`: A40-24Q, driver 580.65.06, fine.
- `docker exec videnoa nvidia-smi`: `Failed to initialize NVML: Unknown Error`;
  `/dev/nvidia*` nodes exist in the container but `/dev/nvidiactl` cannot be
  read.
- Docker 28.2.2, cgroup driver systemd, cgroup v2, runc;
  nvidia-container-toolkit 1.17.8 with `mode = "auto"`; container uses
  `--gpus all` (DeviceRequests only, no `--device`).
- `systemctl show docker-<id>.scope -p DevicePolicy -p DeviceAllow`:
  `DevicePolicy=strict` and no NVIDIA majors (195, 510) in `DeviceAllow`.
  The NVIDIA hook adds the GPU to the cgroup directly at start, so when
  systemd re-applies the scope's device policy (any `daemon-reload`) the GPU
  is dropped. This is NVIDIA's documented "containers lose access to GPUs"
  issue for cgroup v2 + systemd.
- `/dev/char/195:0`, `195:255`, `510:0`, `510:1` symlinks were dated 15:31
  (created around the first host `nvidia-smi` of this investigation); only
  `195:254` dated from container start, so they were absent before.
- Most likely trigger (not confirmed, no journal access): unattended-upgrade
  on 2026-10-07 06:20 UTC (kernel 5.15.0-198, udev-related packages), whose
  maintainer scripts commonly run `systemctl daemon-reload`.
- Only one `videnoa` process on the host (the container's PID 1). The single
  `Failed to persist failed transition ... database is locked` on the first
  failed job is therefore not a second Worker sharing `data/jobs.db`.

## Fix options (none applied)

- Immediate: `docker restart videnoa` restores access until the next
  daemon-reload.
- Permanent, NVIDIA's documented options: create the `/dev/char` symlinks
  persistently (`sudo nvidia-ctk system create-dev-char-symlinks --create-all`
  plus the udev rule from NVIDIA's troubleshooting guide) and pass the nodes
  explicitly with `--device`; or switch to CDI
  (`nvidia-ctk cdi generate`, `--device nvidia.com/gpu=all`); or Docker's
  `native.cgroupdriver=cgroupfs`.
- The container has `RestartPolicy=no`; `/var/run/reboot-required` is set
  (running 5.15.0-191, DKMS has the 580 driver built for 191 and 198).

## Product follow-ups (not done)

- CUDA failure 100 surfaces as "Failed to load ONNX model"; a clearer
  message would point at device visibility.
- The health check stays `healthy` while no GPU is visible.
