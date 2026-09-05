# Videnoa Controller

Videnoa Controller is a standalone, GPU-free coordination service for one or
more remote Videnoa services. It runs beside NAS storage, owns task history in
SQLite, transfers media over Videnoa HTTP APIs, and serves an embedded Web UI.
The existing GPU product remains `videnoa`. A worker record is one remote
Videnoa service instance, not a renamed binary.

An archive contains only the Controller executable, `controller.example.toml`,
this file, and `LICENSE`. The repository has an expanded guide at
`docs/controller.md`, but the steps below are sufficient to install, operate,
back up, restore, and recover an archive deployment.

## First Run

1. Extract the archive and work from its root. Create real, non-symlink input,
   output, data, and temp directories. The Controller must be able to read input
   roots and write the output, data, and temp roots.
2. Copy `controller.example.toml` to an operator-owned path. Replace every path
   with an absolute host path. Keep the input and output root lists narrow.
3. Generate the administrator password hash through hidden terminal input. The
   command prints an Argon2id PHC string, so redirect it straight to a protected
   file and don't record its contents.

Linux:

```bash
install -d -m 0750 /var/lib/videnoa-controller
./videnoa-controller hash-password > /var/lib/videnoa-controller/admin-password.phc
chmod 0600 /var/lib/videnoa-controller/admin-password.phc
./videnoa-controller --config /etc/videnoa-controller.toml
```

Windows PowerShell:

```powershell
New-Item -ItemType Directory -Force C:\VidenoaController | Out-Null
.\videnoa-controller.exe hash-password | Set-Content -NoNewline C:\VidenoaController\admin-password.phc
.\videnoa-controller.exe --config C:\VidenoaController\controller.toml
```

The default listener is `127.0.0.1:3001`. `--host` and `--port` override the
configuration for that process. Check the binary and public liveness endpoint:

```bash
./videnoa-controller --version
curl --fail http://127.0.0.1:3001/api/health
```

Expected health response: `{"status":"ok"}`. Sign in through the Web UI, open
Workers, and register each credential-free Videnoa base URL with its compute
slot count. A workflow is schedulable only after that service is online and
reports a workflow or preset with `Path` inputs named exactly `input` and
`output`. Controller doesn't copy or deploy workflows.

Create tasks through the Web UI or authenticated `POST /api/tasks`. These are
the only intake paths. ANI-RSS and other automation call that generic HTTP POST.
There is no watcher, polling adapter, qBittorrent integration, cron discovery,
or rules engine.

## Security

Keep `secure_cookie = true` behind HTTPS. Tailscale provides private routing,
but use HTTPS when browsers or untrusted networks can reach the listener. Set
`secure_cookie = false` only for a deliberate trusted HTTP deployment. Startup
prints a warning in that mode.

The hash file must be a regular, non-symlink file readable by the Controller.
Configuration contains only its path. Browser sessions use an HttpOnly,
SameSite=Strict cookie and CSRF proof. API clients send the same administrator
password as `Authorization: Bearer ...`; never place it in a URL, TOML file,
shell history, logs, or reusable curl example. Keep secret files out of source
control. Root allowlists still apply after authentication.

Rotate the password by generating a replacement hash into a new protected file,
atomically replacing the configured hash file, then confirming a new login.
The service reloads the file for login, Bearer, and session checks. Existing
sessions and the old Bearer password become invalid without a restart.

## Data and Backup

`data_root/controller.sqlite3` is durable Controller truth. SQLite uses WAL, so
stop the Controller cleanly before a simple file backup, then copy the whole
`data_root`, including `controller.sqlite3`, `controller.sqlite3-wal`, and
`controller.sqlite3-shm` when present. Also preserve the config and password
hash file. `temp_root` contains restart-recovery artifacts and should remain
persistent during normal operation, but it is not a substitute for the database.
It must be disjoint from every output root and on the same filesystem as each
one; Controller publishes by atomic no-replace rename and never copies through
an output-root staging file.

Every remote Videnoa service must persist its own data directory, especially
`jobs.db` and the task workspaces under `workspace/`. That evidence is required
for keyed reconciliation. Losing `jobs.db` while Controller work refers to the
service creates ambiguity. It never authorizes blind resubmission.

Restore only while the Controller is stopped. Restore the data directory,
config, hash file, NAS roots, and the matching persistent Videnoa data before
starting the same or a compatible Controller version. Check liveness,
authenticated readiness, Workers, and active task details before resuming new
work.

## Critical Recovery

SQLite is authoritative. SSE and the Web UI are views of API state, not recovery
evidence. On restart, Controller scans nonterminal rows and resumes the durable
stage. Don't delete `controller.sqlite3`, Videnoa `jobs.db`, temp artifacts,
legacy destination staging files named by durable evidence, final outputs,
attempts, or remote workspaces to make a task move.

For `remote_state_ambiguous`, disable the affected worker to stop new assignment.
Preserve the Controller database, the worker's `jobs.db`, workspace, logs, task
ID, attempt ID, remote job ID, and exact workflow parameters. Compare that
evidence before any manual action. Don't retry or create a replacement task that
could repeat unknown compute.

For `publication_ambiguous`, pause scheduling and preserve the requested final
path, the verified artifact under `temp_root`, and any legacy hidden staging
file named by durable evidence. Compare their length and SHA-256 against the
task record. Don't delete, rename, overwrite, or force retry any artifact.
Existing and racing destinations are never replaced.

Cancellation is available from queued through verifying. Publishing,
remote-cleanup, completed, failed, and cancelled tasks reject cancellation.
Retry is stage-aware. A processing retry creates a new attempt only after remote
terminal identity and workspace cleanup are proven. Download, verification,
publication, and cleanup retries keep the existing attempt and never rerun
successful AI processing. Ambiguous failures are not retryable.

## Upgrade and Rollback

Before an upgrade, pause scheduling, let active work reach a stable state, stop
the process, and back up Controller and worker data. Start the new executable or
image with the same mounts and config. SQLx migrations run automatically at
startup. Verify `/api/health`, authenticated `/api/readiness`, worker state, and
several retained task details before resuming.

Rollback requires the pre-upgrade data snapshot because an older binary isn't
promised to understand newer migrations. Stop the new version, preserve its data
for diagnosis, restore the full pre-upgrade Controller and matching worker data,
then start the previous binary or image. Never point an older binary at a
database already migrated by a newer version.

## Distribution

Published images are `controlnet/videnoa-controller:<version>` and
`controlnet/videnoa-controller:latest`. Pin the version tag for production.
The Linux archive is `videnoa-controller-v<version>-linux-x86_64.tar.gz`; the
Windows archive is `videnoa-controller-v<version>-windows-x86_64.zip`.

The container runs as UID/GID `10001:10001`. Mount config read-only, Controller
data and temp read-write, input roots read-only, output roots read-write, and the
hash file read-only. Host permissions must still let UID 10001 read that file.
No GPU flags, models, ONNX Runtime, CUDA, cuDNN, or TensorRT libraries belong in
the Controller deployment.

## Troubleshooting

- Startup rejects a root: create the directory first, remove symlinks from the
  path, and confirm the service account has the required access.
- Startup rejects the password file: confirm it is a readable regular file that
  contains one Argon2id PHC string generated by `hash-password`.
- Health works but readiness fails: authenticate and inspect the `migrations`,
  `authentication`, and `root_handles` checks from `/api/readiness`.
- A worker stays offline or incompatible: verify its credential-free HTTP(S)
  URL, Videnoa health, persistent data, and exact workflow interface.
- Tasks don't start: check scheduler pause, worker enabled/online state,
  workflow compatibility, compute slots, transfer limits, and task failure data.
- Output already exists: preserve it. Choose a different `output_path` and
  create a new task. Controller has no overwrite or auto-rename fallback.
