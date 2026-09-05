# Videnoa Controller

Videnoa Controller is a standalone, GPU-free coordination service for remote
Videnoa workers. It serves an embedded Web UI, stores durable task history in
SQLite, transfers media through worker HTTP APIs, and resumes nonterminal work
after restart. It does not replace the GPU application named `videnoa`.

The archive contains only the Controller executable, `controller.example.toml`,
this guide, and `LICENSE`. No system installation or administrator account is
required.

## First Run

Extract the archive into the directory that will be the Controller workspace,
open a terminal in that directory, and run the executable directly.

Linux:

```bash
./videnoa-controller
```

Windows PowerShell:

```powershell
.\videnoa-controller.exe
```

The default listener is `127.0.0.1:3001`. On first start Controller creates only
`./data/controller.toml` and `./data/controller.sqlite3`. SQLite may also create
`controller.sqlite3-wal` and `controller.sqlite3-shm`, and active tasks may use
transient UUID-named directories under `./data`. Controller does not create
generic input, output, config, secret, or authentication directories.

Open `http://127.0.0.1:3001/`. The first Web UI asks for the administrator
password and confirmation. The password must contain at least 12 bytes. Its
Argon2id hash is stored in SQLite; the password and hash are not written to TOML
or a separate credential file. A second setup attempt is rejected.

The setup API used by the Web UI is:

- `GET /api/auth/setup` returns `{"initialized":false}` or
  `{"initialized":true}`.
- `POST /api/auth/setup` accepts `password` and `password_confirmation`, requires
  an exact `Origin` matching the request scheme and Host, and returns the normal
  login session cookie and CSRF response when successful.

Check the public liveness endpoint without exposing credentials:

```bash
curl --fail http://127.0.0.1:3001/api/health
```

Expected response: `{"status":"ok"}`.

## Workspace and Media Paths

Task `input_path` and `output_path` values define where media lives. Relative
paths resolve from the directory where the binary was started. Absolute paths
must still resolve within that workspace. The entire `./data` subtree is private
Controller storage and cannot be used for task input or output.

There are no fixed input or output directories and no media-path configuration.
For example, an operator may place an input at `media/incoming/episode.mkv` and
request `media/library/episode.mp4`; another task may choose different
directories in the same workspace. Parent traversal, symlink escape, non-regular
input, changed input identity, and an existing output fail closed. Publication
never overwrites or auto-renames an existing destination.

Register each credential-free Videnoa worker URL in the Web UI. A workflow is
eligible only when the worker reports a workflow or preset with `Path` inputs
named exactly `input` and `output`. Controller does not copy or deploy worker
workflows.

## Settings and Security

The generated `./data/controller.toml` and the shipped example contain only
`server`, `auth`, `scheduler`, `timeouts`, and `retry`. Unknown fields are
rejected. Defaults are loopback port 3001, non-Secure cookies for trusted local
HTTP, 24-hour absolute sessions, one-hour idle sessions, one compute slot, one
prefetched task, one upload, one download, health/poll/transfer timeouts of
10/5/300 seconds, retry delays of 1 through 60 seconds, and five attempts.

Web UI Settings persists every public field to SQLite and projects the current
configuration back to `./data/controller.toml`. Server, cookie/session policy,
scheduler, timeout, and retry changes are hot-applied. A server address change
must be bindable before the update is accepted.

Keep the default loopback listener unless another network layer requires remote
access. `secure_cookie = false` is for deliberate trusted HTTP use only. Use
same-origin HTTPS and `secure_cookie = true` when browsers can reach Controller
through an untrusted network. Browser sessions use an HttpOnly,
SameSite=Strict cookie plus CSRF proof. Never put passwords, Authorization
headers, cookies, or CSRF values in URLs, configuration, logs, or source control.

After setup, `POST /api/auth/login` accepts the administrator password. API
clients may use the same password as a Bearer credential, but should read it
through protected interactive input rather than a command argument or reusable
script.

## Data and Backup

SQLite is authoritative. Stop Controller cleanly before a simple filesystem
backup, then copy the complete `./data` directory. Include SQLite WAL sidecars
and transient task directories when present. The administrator hash, settings,
sessions, tasks, attempts, idempotency, retry state, and recovery evidence are
inside this durable data.

Preserve task-defined source and destination media separately according to your
normal storage backup policy. Every worker must also preserve its own Videnoa
data, especially `jobs.db` and task workspaces. Losing worker job identity while
Controller has active work creates `remote_state_ambiguous`; Controller does not
authorize blind resubmission.

Restore only while Controller is stopped. Restore the full `./data` snapshot,
matching media, and matching worker data into the same workspace layout. Start
the same or a compatible Controller version, then inspect health, authenticated
readiness, Workers, and every nonterminal task before resuming scheduling.

## Critical Recovery

SSE and the Web UI are views of API state, not recovery evidence. Do not delete
`controller.sqlite3`, SQLite sidecars, transient task data, final outputs,
attempts, or remote workspaces to make a task move.

For `remote_state_ambiguous`, disable the affected worker and preserve Controller
data, the worker's `jobs.db`, workspace, logs, task ID, attempt ID, remote job ID,
and exact workflow parameters. Compare identity evidence before any manual
action. Do not retry or submit equivalent replacement work while the outcome is
unknown.

For `publication_ambiguous`, pause scheduling and preserve the requested final
path and verified transient artifact. Compare regular-file type, length, and
SHA-256 with the task record. Do not delete, rename, overwrite, or force retry an
unknown artifact.

Cancellation is available from queued through verifying. Publishing,
remote-cleanup, and terminal tasks reject cancellation. Retry is stage-aware:
downstream transfer/publication retries do not rerun successful AI processing,
and ambiguous failures are not retryable.

## Upgrade and Rollback

Before upgrading, pause scheduling, let work reach known states, stop Controller,
and back up the complete Controller and worker data. Replace only the executable
or image, keep the workspace, and start the new version. Database migrations run
at startup.

Rollback requires the pre-upgrade data snapshot. An older binary is not promised
to understand a database migrated by a newer version. Stop the new version,
preserve its data for diagnosis, restore the matching pre-upgrade Controller and
worker snapshots, then start the previous version.

## Docker

The image runs from `/workspace`, preserves default `USER 10001:10001`, and
declares no config, secret, input, or output volumes. Bind one writable host
workspace that is the common parent of media and `data`. Running with the host
user avoids root-owned files and does not require changing host ownership:

```bash
mkdir -p controller-workspace
docker run -d --name videnoa-controller \
  --user "$(id -u):$(id -g)" \
  -p 127.0.0.1:3001:3001 \
  --mount "type=bind,src=$(pwd)/controller-workspace,dst=/workspace" \
  controlnet/videnoa-controller:<version> --host 0.0.0.0
```

The explicit `--host 0.0.0.0` is required for container port forwarding and
binds every container interface. Keep the published host port on loopback for
trusted first setup, or protect it with firewalling and a same-origin HTTPS
reverse proxy. Do not expose an uninitialized setup endpoint to an untrusted
network.

No GPU flags, models, ONNX Runtime, CUDA, cuDNN, or TensorRT libraries belong in
the Controller deployment.

## Distribution

Published images are `controlnet/videnoa-controller:<version>` and
`controlnet/videnoa-controller:latest`. Pin a version in production. Archives are
`videnoa-controller-v<version>-linux-x86_64.tar.gz` and
`videnoa-controller-v<version>-windows-x86_64.zip`.

## Troubleshooting

- Startup cannot create `./data`: confirm the current workspace is writable by
  the process or container user.
- Setup returns `403`: send the exact same-origin scheme and Host in `Origin`.
- Setup returns `400`: use matching password fields with at least 12 bytes.
- Setup returns `409`: the administrator credential is already initialized; use
  login rather than trying to replace it through setup.
- Health works but readiness fails: finish administrator setup, authenticate,
  and inspect readiness before admitting tasks.
- A path is rejected: keep task media inside the workspace and outside `./data`,
  remove traversal and symlink components, and preserve existing destinations.
- A worker stays offline or incompatible: verify its credential-free HTTP(S)
  URL, Videnoa health, persistent data, and exact workflow interface.
- Output already exists: preserve it and create a new task with another path.
