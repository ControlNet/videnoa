# nectar3: videnoa container lost the GPU while running (2026-10-08)

Access: `ssh -i ~/.ssh/id_rsa_ansr nectar3.neusym.cloud.edu.au` (host
`ansr-nectar-3`, user `zhixi`, groups `sudo docker`, TZ Etc/UTC). Passwordless sudo was
added by the user on 2026-10-08 in `/etc/sudoers.d/90-zhixi-nopasswd`, so
`sudo -n` works over non-interactive SSH.

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
- Confirmed trigger: `sudo journalctl _PID=1` shows the only systemd reloads
  between container start and the failures at 2026-10-08 06:10:48, 06:10:52
  and 06:10:59 UTC. `/var/log/apt/history.log` shows `unattended-upgrade`
  upgrading `snapd` (2.76 -> 2.76.3) from 06:10:38 to 06:11:00; its
  maintainer scripts stopped/restarted snapd and reloaded systemd. The
  2026-10-07 06:20 run (kernel 5.15.0-198) caused no reload. The failures
  only showed at 15:25 because that was the first GPU job after the reload.
- Only one `videnoa` process on the host (the container's PID 1). The single
  `Failed to persist failed transition ... database is locked` on the first
  failed job is therefore not a second Worker sharing `data/jobs.db`.

## Actions taken (2026-10-08 ~15:45 UTC)

- Automatic installs disabled without touching systemd units
  (`systemctl enable/disable` itself triggers a daemon-reload):
  `/etc/apt/apt.conf.d/20auto-upgrades` now sets
  `APT::Periodic::Update-Package-Lists "1"` and
  `APT::Periodic::Unattended-Upgrade "0"`; original saved as
  `/var/backups/20auto-upgrades.bak-2026-10-08` (a backup inside
  `apt.conf.d/` makes apt print an "invalid filename extension" notice).
  Verify: `apt-config dump | grep Periodic::Unattended-Upgrade` -> `"0"`.
- Snap auto-refresh held: `sudo snap refresh --hold` (`snap refresh --time`
  shows `hold: forever`). Snap refreshes rewrite mount units and reload
  systemd too. Undo: `sudo snap refresh --unhold`.
- `docker restart videnoa`; `docker exec videnoa nvidia-smi -L` lists
  `GPU 0: NVIDIA A40-24Q`, health `healthy`.
- Consequence: no automatic security updates. After manual
  `sudo apt upgrade`, run `docker restart videnoa` and check
  `docker exec videnoa nvidia-smi -L`.
- Undo apt change:
  `sudo cp -a /var/backups/20auto-upgrades.bak-2026-10-08 /etc/apt/apt.conf.d/20auto-upgrades`.

## Remaining fix options (not applied)

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
