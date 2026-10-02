## What's new

- **Untagged sources decode as BT.709 at every size.** v0.1.7 still decoded untagged SD-sized frames (narrower than 1280 and at most 576 high) as BT.601. Tagged sources keep using their tag.
- **Invalid workflows are rejected when submitted.** A frame chain with a missing required input (for example a `VideoOutput` without `output_path`), the wrong topology, or a node that cannot run in it now returns 400 from `POST /api/jobs`, `POST /api/run` and `videnoa run` before a job is queued. Previously such jobs were accepted and failed later with a vaguer error. `POST /api/batch` is now all-or-nothing.
- **Controller shows why a Worker rejected a task.** The task's failure message now ends with the Worker's reason, such as `node 'output' missing required input port 'output_path'`.

## Fixes and reliability

Worker

- **NVENC on the A40 and similar GPUs:** the NVENC availability probe encoded a 64x64 frame, which some GPUs reject, so NVENC was reported unavailable. It now encodes 256x256, is stopped after 30 s, and successful results are remembered.
- **Clearer encoder errors:** encoder failures quote the end of FFmpeg's own error output instead of only "exited with status N" or "Broken pipe". Errors from output nodes are reported directly instead of being replaced by a later error.
- **Live streams:** `StreamOutput` encodes 8-bit 4:2:0 H.264, which RTMP ingest servers accept.
- **More accurate decoding:** YUV to RGB conversion in jobs and previews uses accurate rounding (round-trip error about ±2 instead of ±3).
- **Frame interpolation:** if pinned host memory cannot be allocated, RIFE falls back to ordinary memory instead of failing the job.
- **Wide-gamut sources:** BT.2020, DCI-P3 and XYZ primaries are still not converted, but the Worker now logs a warning.
- **Previews:** long or 4K sources no longer time out while frames are counted, preview subprocesses are bounded, and the preview samples the same video stream a job decodes.
- **HTTP server:** connections that do not finish sending a request head within 30 seconds are closed. The server speaks HTTP/1.1 only.
- `videnoa run` stores TensorRT engines in `paths.trt_cache_dir` like the server.
- API-created presets are capped at 256 (`409 preset_limit_reached`); restart the Worker to clear them.
- The editor palette no longer offers `ColorSpace` and `SceneDetect`, which could not run in a video job.

Controller

- **Worker connections:** idle connections to Workers are dropped after 20 seconds, before the Worker closes them. Reusing a connection just as the Worker closed it could fail a health probe and briefly mark the Worker offline.
- **Login limiting:** the failure limit is checked before the password is verified. After five failed attempts within five minutes, a client receives 429 until the window passes, even with the correct password, and no longer uses up password verification capacity.
- **Sessions:** expired sessions are purged periodically instead of accumulating.
- **Several tabs:** a tab whose CSRF proof was rotated by another tab refreshes it and retries instead of failing with 403.
- **Sleep and resume:** the live event stream is checked when the page resumes, so a stalled connection no longer shows stale data as connected.
- **Fewer database reads:** the worker list loads capacity in one query, and live updates read each change once for all open pages. The database pool's acquire timeout is separate from the SQLite busy timeout.
- Changing only the listener host on the same port returns a clear error (change the port too, or restart).

Security and dependencies

- `rustls` updated to 0.23.45 (RUSTSEC-2026-0285). npm advisories in the Worker and Controller web apps are cleared.

## Downloads and Docker

Linux and Windows Worker bundles and standalone Controller archives are available below. Download all parts of a split Worker bundle before extracting the `.7z.001` file.

- Worker: `controlnet/videnoa:0.1.8`
- Controller: `controlnet/videnoa-controller:0.1.8`

Both image repositories also publish `latest`. The Controller does not require a GPU. The Worker image now ships the same FFmpeg 8.1 build as the release archives instead of the distribution's FFmpeg 4.4.

## Upgrade

Back up Worker configuration and Controller data before upgrading. Keep the same persistent directories when replacing containers.

- Untagged SD sources now decode as BT.709 instead of BT.601, so their colours differ from v0.1.7.
- Workflows with an invalid frame chain fail at submission instead of during the job. In the Controller, such a task fails as `remote_submission_failed` and cannot be retried; fix the workflow on the Worker and create a new task.
- Upgrade the Controller together with the Workers. A v0.1.7 Controller with a v0.1.8 Worker can occasionally mark the Worker offline for about a second.
- Clients must use HTTP/1.1; cleartext HTTP/2 (h2c) is no longer accepted. HTTPS reverse proxies are unaffected.

See the [Worker and Docker instructions](https://github.com/ControlNet/videnoa/blob/v0.1.8/README.md), [Controller archive and Docker guide](https://github.com/ControlNet/videnoa/blob/v0.1.8/README-controller.md), and [Controller reference](https://github.com/ControlNet/videnoa/blob/v0.1.8/docs/controller.md).

**Full changelog:** https://github.com/ControlNet/videnoa/compare/v0.1.7...v0.1.8
