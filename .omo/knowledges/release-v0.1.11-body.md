## What's new

- **Tasks on a Worker that is gone for good can be ended.** In the task detail, **Abandon** ends a task whose cancellation is waiting for a Worker that no longer answers: the task fails with `worker_unavailable`, its Controller temporary files and its Worker capacity are released, and nothing is sent to the Worker. A Worker can now be deleted once all of its tasks are completed, failed or cancelled; finished attempts keep its name in the history. **Discard** gives up on a failed task: it can no longer be retried, its temporary files are released, and its folder on the Worker is deleted when the Worker confirms the job ended. The Web UI offers exactly the actions each task lists in its new `allowed_actions`.
- **The Controller releases disk it no longer needs.** A task that fails in a way no retry can resume releases its Controller temporary files, including a downloaded output, as soon as the failure is recorded, and startup releases any such files a crash left behind. The new optional `[history] retention_days` in `controller.toml` purges finished tasks after that many days; it is off by default.
- **Worker problems no longer fail tasks or stop the Controller.** A Worker that rejects the saved password, rate-limits, answers through a proxy with a 404 or redirect, or reports a health or job status the Controller does not know now holds that Worker's tasks until it recovers. Before, every processing task on it failed and could be neither retried nor cancelled while its GPU job kept running, and an unexpected health reply during recovery stopped the Controller. Only evidence about the job itself fails a task.
- **Controller and Worker share a protocol version.** Worker `/api/health` reports `"protocol": 2`. Workers tag their rejections with a code, so a submission that retrying cannot fix (an unreadable or invalid workflow, a missing job store) fails at once with the Worker's reason instead of being retried forever, while a full queue or a Worker that is shutting down is retried. A Worker on a newer protocol is shown offline with both versions named.
- **The Worker stops cleanly.** `docker stop` (SIGTERM) and Ctrl-C refuse new jobs, cancel running ones (reported as `cancelled` with `cancel_reason` `shutdown`), wait up to 6 seconds and exit. The Worker image runs videnoa under tini, so SIGTERM reaches it. Only one Worker can use a data directory at a time.
- **The workflow editor no longer overwrites or drops work.** Save writes to the file that is loaded, saving under a new name asks before replacing an existing file, unsaved changes are confirmed before loading, clearing or closing, and undo no longer brings back another workflow's canvas.

## Fixes and reliability

Worker

- **A passwordless Worker checks the Host name (DNS rebinding).** While no password is set, the Worker accepts a request only when its `Host` is an IP address, `localhost` or a `*.localhost` name, the machine's own host name, or a name in the new `server.allowed_hosts`. Before, a web page that rebinds its own domain to the Worker's address could save workflows, browse files and set the first password. See Upgrade.
- **A peer without the password can no longer block the iroh tunnel.** Rejected tunnel requests are now answered within the handshake timeout, so a peer that stalls them no longer holds stream slots until no tunnel, including the Controller's, can open.
- **The Jellyfin API key no longer follows a redirect to another host.** Redirects are followed only within the same origin or from http to https on the same host; any other redirect fails and names the target host.
- **Cancel keeps the job visible until it has stopped.** A cancelled queued or running job stays listed with `cancel_requested` until its work has ended and the GPU is free, then becomes `cancelled`. Before, it disappeared at once while it could still hold the GPU and block the next job. Cancelling also stops a job that is still loading its models; a TensorRT engine build already under way finishes first.
- **Admission is bounded.** At most 64 jobs can be queued or running; further submissions get 503 `queue_full`. A preview waits at most 2 seconds for the GPU, then answers 503. Jobs made only of nodes that do not use the GPU no longer wait for it. `/api/health` reports `gpu_slots`, `running`, `queued` and `cancelling`.
- **Validation matches what runs.** A node param the node does not know is rejected, where a renamed or misspelled param used to run silently with its default. Scalar nodes can use a video node's outputs: those fed by VideoInput run before the stream, and those fed by VideoOutput (a "done" webhook, a Print) run after the output is published and not when the job fails or is cancelled. A Workflow node that points at a video workflow is rejected at submission.
- **`device_id` on SuperResolution and FrameInterpolation is used.** Setting it to 1 used to run on GPU 0.
- **TensorRT engines are cached per TensorRT and ONNX Runtime build**, so an engine left by another build is no longer loaded. For models with fixed input sizes, an engine that cannot be loaded is moved aside and rebuilt once.
- **Saving a workflow cannot silently replace another.** It writes the file the editor names, refuses to replace an existing file unless asked (409 `workflow_exists`), and refuses a name that is a preset's run name (409 `workflow_name_is_preset`), which would have replaced that preset in `/api/run` and in Controller tasks. Saved workflows record a `schema_version`; a newer one is refused with a message to upgrade.
- **The Worker refuses to start without its job store.** Before, it ran with in-memory jobs only and still reported healthy, so a restart lost every job a Controller was polling. `jobs.db` and `config.toml` now record a version, rows that cannot be read are skipped with their IDs logged and can be deleted, and rerun validates the stored workflow like a new submission.
- **Relative paths have fixed anchors.** `workflows_dir` is relative to the data directory, `models_dir`, `presets_dir` and `trt_cache_dir` to the first of the working directory, the executable's directory and its parent that contains `presets_dir`, and a relative `model_path` in a workflow is looked up in `models_dir`.
- A job that panics is recorded as failed instead of staying `running` until restart.
- Settings carry an ETag, so the Settings page and the desktop language switch no longer revert each other.
- `GET /api/presets` lists `paths.presets_dir` as it is now, not as it was at startup.
- Print values are sent to a progress connection that opens after they fired.
- Hidden staged output files of jobs that were running when the Worker stopped are removed on the next start.

Controller

- **Cancelling a submission whose reply was lost looks the job up** on the Worker instead of sending the run again, so no job is created just to be cancelled.
- **A cancelled task waits until the Worker has stopped its job** before its Worker folder is deleted, and a job the Worker reports cancelled says who cancelled it.
- **One unreadable workflow or preset no longer takes the Worker offline.** It is listed as invalid with a reason, and the Worker keeps running the others. A preset that a saved workflow of the same name hides is listed as invalid too.
- A Worker that refuses the Controller's Host name shows the Worker's reason instead of blaming the saved password.
- A Worker whose tunnel version differs is reported as incompatible ("update the worker or the Controller") instead of as an ordinary outage.
- A rejection about one task's job backs off only that task, not the whole Worker.
- A job in a status the Controller does not know keeps being polled instead of failing its task.

Worker WebUI

- Deleting a saved workflow asks for confirmation.
- Failed cancel, retry and history delete on the Jobs page show the server's message.
- The editor shows previews only for the job its own canvas submitted, and drops a deleted job's previews.
- Nodes and params the Worker does not know are shown with a remove button, and params keep their JSON values instead of being turned into strings.
- Failures that showed as empty lists or nothing now show an error with Retry: node definitions, the workflow gallery, the workflow picker and folder listings. Node definitions reload when the window regains focus.
- A password change in one tab no longer breaks saving in other tabs.
- The remaining English-only texts are translated.
- On the desktop app, the server host and port settings are marked not applicable.

Controller WebUI

- **Abandon** and **Discard** are in the task detail, each with a confirmation.
- A field the Web UI does not know no longer hides the whole task or worker list, and a row it cannot read is left out instead of failing the list.

Build and dependencies

- `npm audit fix` in both web apps: zod 4.6.7 (GHSA-mrhx-c7xc-m3w5, the only one in a production bundle), vite 7.3.7, vitest 4.1.12, undici 7.30.1 and source-map-js 1.2.2. Both report no vulnerabilities.
- The Worker image uses tini as its entrypoint and `STOPSIGNAL SIGTERM`.

## Downloads and Docker

Linux and Windows Worker bundles and standalone Controller archives are available below. Download all parts of a split Worker bundle before extracting the `.7z.001` file.

- Worker: `controlnet/videnoa:0.1.11`
- Controller: `controlnet/videnoa-controller:0.1.11`

Both image repositories also publish `latest`. The Controller does not require a GPU.

## Upgrade

Back up Worker configuration and data, and Controller data, before upgrading. Keep the same persistent directories when replacing containers. Upgrade the Controller first, then the Workers: a v0.1.11 Controller works with v0.1.9 and v0.1.10 Workers, while a v0.1.9 or v0.1.10 Controller does not understand a v0.1.11 Worker's error codes, cancel reasons or Host refusal.

- **Passwordless Workers accept only known Host names.** Reaching a Worker by IP address, `localhost` or its own host name works as before. A Worker without a password that is reached, or registered in the Controller, by any other DNS name answers 403 until that name is added to `server.allowed_hosts` in `config.toml` (without scheme or port) or a password is set. Inside Docker the container has its own host name, so this applies to every DNS name there, including a Compose service name. A v0.1.11 Controller shows "worker refused this Controller" with the Worker's reason; an older Controller reports it as a wrong Worker password and stops checking that Worker until its registration is edited.
- **Workflows with unknown node params are rejected** with 400 (`workflow validation failed: node '...' has unknown param '...'`), and the Controller lists such a workflow as invalid. Open it in the editor and remove the params listed as unsupported.
- **`device_id` is honoured.** A workflow that set it to anything other than 0 now runs on that GPU.
- **TensorRT engines are rebuilt once** on first use after the upgrade, because the cache key now includes the TensorRT and ONNX Runtime versions. The old entries stay in `trt_cache` until you delete them.
- **`config.toml` gains `version = 2` and `server.allowed_hosts`.** A relative `workflows_dir` is now relative to the data directory. An existing file is converted when read: the old default `data/workflows` becomes `workflows`, and any other relative value is pinned to the working directory it meant. Workflows in `./data/workflows` are copied into a custom data directory on first start; the originals stay. The file is rewritten only when settings are saved. After that, v0.1.10 looks for saved workflows in `<working directory>/workflows` instead, so restore the backed-up `config.toml` to go back.
- The Worker job store (`jobs.db`) moves to schema version 2; v0.1.10 can still open it. Saved workflows gain `"schema_version": 1`, which v0.1.10 ignores.
- **A second Worker on the same data directory exits** with an error, and every data directory now contains `instance.lock`. A Worker whose job store cannot be opened no longer starts. `docker run --entrypoint` bypasses tini, so SIGTERM then does not reach the Worker.
- The Controller migrates its database on first start (migration 0015 keeps a deleted Worker's name on its attempts, 0016 records discarded tasks). A v0.1.10 Controller refuses to open the migrated database, so restore the backup to go back.
- Worker HTTP API: `POST /api/presets` is removed (405). `POST /api/workflows` accepts `filename` and `overwrite`; without `overwrite: true` it no longer replaces an existing file (409 `workflow_exists`), and a preset's run name gets 409 `workflow_name_is_preset`. `GET /api/config` returns an `ETag`, and `PUT /api/config` with a stale `If-Match` gets 412 `config_changed`; without `If-Match` the write is unconditional, as before.
- `DELETE /api/jobs/{id}` on a queued or running job keeps it listed with `cancel_requested`, `cancel_requested_at` and `cancel_reason` until it stops. Error replies carry a `code` next to `error`, `/api/health` reports `protocol`, `gpu_slots`, `running`, `queued` and `cancelling`, job submissions beyond 64 active jobs get 503, and `GET /api/jobs/by-idempotency-key/{key}` is new.
- Controller API: `POST /api/tasks/{id}/abandon` and `POST /api/tasks/{id}/discard` take the current `version`, and every task lists `allowed_actions`.
- Deleting a Worker sets `worker_id` to null on its finished tasks and attempts, and the attempts keep its name in `worker_name`. A failed task whose Worker was deleted cannot be retried; create a new task instead.
- Batch `Idempotency-Key`s expire seven days after the first request; the key then starts a new batch.
- `[history] retention_days` is edited only in `controller.toml` and applies after the next Controller start.
- v0.1.11 Workers need Controller v0.1.9 or later, because Controllers up to v0.1.8 reject the new fields in health and job replies.

See the [Worker and Docker instructions](https://github.com/ControlNet/videnoa/blob/v0.1.11/README.md), [Controller archive and Docker guide](https://github.com/ControlNet/videnoa/blob/v0.1.11/README-controller.md), [Controller reference](https://github.com/ControlNet/videnoa/blob/v0.1.11/docs/controller.md) and [iroh guide](https://github.com/ControlNet/videnoa/blob/v0.1.11/docs/iroh.md).

**Full changelog:** https://github.com/ControlNet/videnoa/compare/v0.1.10...v0.1.11
