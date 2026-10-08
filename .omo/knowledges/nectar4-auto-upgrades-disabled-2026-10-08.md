# nectar4 (local dev host): auto-upgrades disabled, CDI keeps GPUs across reloads (2026-10-08)

Host `ansr-nectar-4`, Ubuntu 22.04.5, A40-24Q. Same change as nectar3 (see
`nectar3-docker-gpu-lost-after-daemon-reload-2026-10-08.md`).

## Actions taken

- `/etc/apt/apt.conf.d/20auto-upgrades` now sets
  `APT::Periodic::Update-Package-Lists "1"` and
  `APT::Periodic::Unattended-Upgrade "0"`; original in
  `/var/backups/20auto-upgrades.bak-2026-10-08`. No `systemctl` call was made.
  Verify: `apt-config dump | grep Periodic::Unattended-Upgrade` -> `"0"`.
- `sudo snap refresh --hold` (`snap refresh --time` shows `hold: forever`).
- Undo:
  `sudo cp -a /var/backups/20auto-upgrades.bak-2026-10-08 /etc/apt/apt.conf.d/20auto-upgrades`
  and `sudo snap refresh --unhold`.

## Why nectar4's GPU container survived the same reloads

systemd reloaded 3 times at 2026-10-08 06:18 UTC, after the `whisper`
container (`--gpus all`, `RestartPolicy=unless-stopped`) had started at
00:44, yet `docker exec whisper nvidia-smi -L` still listed the GPU.

| | nectar3 (lost GPU) | nectar4 (kept GPU) |
|---|---|---|
| Docker / runc | 28.2.2 / 1.2.5 | 29.2.1 / 1.3.4 |
| nvidia-container-toolkit | 1.17.8 | 1.18.2 |
| CDI spec | none | `/var/run/cdi/nvidia.yaml`, generated at boot by `nvidia-cdi-refresh.{path,service}` (enabled) |
| Scope `DeviceAllow` NVIDIA entries | none | `/dev/char/195:0`, `195:254`, `195:255`, `509:0`, `509:1` |

With a CDI spec present, `--gpus all` puts the NVIDIA device nodes in the
container's device list, so systemd's scope keeps them on a daemon-reload.
On nectar3 the legacy hook adds them behind systemd's back, so a reload
removes them. Upgrading nectar3 to toolkit >= 1.18 (with the CDI refresh
units enabled) and a Docker version that resolves `--gpus` through CDI, or
running `sudo nvidia-ctk cdi generate --output=/etc/cdi/nvidia.yaml` and
recreating the container with `--device nvidia.com/gpu=all`, should make it
immune; not yet verified on nectar3.
