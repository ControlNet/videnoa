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
  a valid same-host HTTP(S) `Origin` (HTTPS required with Secure cookies), and returns the normal
  login session cookie and CSRF response when successful.

Check the public liveness endpoint without exposing credentials:

```bash
curl --fail http://127.0.0.1:3001/api/health
```

Expected response: `{"status":"ok"}`.

## Workspace and Media Paths

Task paths may refer to any safe filesystem location visible to the Controller
process. Absolute paths retain their OS location, for example
`/mnt/user/media/anime/Frieren/E08.mkv` and
`/mnt/user/media/anime/Frieren/E08.AI.mp4`; they are never rebased under workspace.
Relative paths resolve from the Controller workspace (the startup working
directory). With workspace `/opt/videnoa-controller`, `media/E08.mkv` resolves to
`/opt/videnoa-controller/media/E08.mkv`. Task records store normalized absolute
paths. The workspace is only Controller's working location, not a media sandbox.
The entire `<workspace>/data/**` subtree is private and forbidden for task input,
output, and recovery capabilities, including indirect symlink paths.

Input must be a regular file. Parent traversal, unsafe symlink components, changed
input identity, and existing or racing output fail closed. Input and output
extensions may differ.

Downloaded bytes are verified in private UUID task directories under `data`.
Publication first attempts atomic no-replace rename. If it returns `EXDEV`
(including separate bind mounts on the same host disk), Controller falls back to
move semantics: exclusively create the requested final file, copy and fsync its
bytes, verify its size/SHA-256, then remove the private source. Absolute output
paths on other filesystems are accepted at intake.

**During the copy fallback, the final filename is visible before copying finishes.**
Jellyfin or another scanner may observe that incomplete file. Same-mount atomic
rename retains the complete-file visibility guarantee. Neither route overwrites
an existing destination, changes the requested path, or creates sibling staging
files such as `.videnoa-*`, `.partial`, or `.staging`.

Private copy evidence records the exclusively created output's file identity.
After interruption, Controller validates that identity and the existing byte
prefix before appending the remainder from the verified source. A replaced or
corrupt output, or missing ownership evidence, fails as publication ambiguity;
the source is retained. Successful publication recovery never repeats AI compute.
Upgrades make legacy cross-mount publication failures with verified-output evidence
retryable; use Retry to resume publication on the same attempt. They do not retry
automatically.

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

Plain HTTP on a trusted LAN is a supported deployment, including access through
a LAN IP address or hostname. Setup, login, task operations, Workers, Settings,
and live updates work with the default `secure_cookie=false`; HTTPS and public
Internet exposure are not required. Leave **Require secure session cookie** off
for HTTP deployments. That option explicitly requires HTTPS for session use.

`data/controller.toml` is the sole persisted Controller configuration source.
The in-memory `ControllerConfig` is the active runtime configuration.
`controller.sqlite3` holds durable operational/application state: tasks, attempts,
workers, recovery evidence, idempotency, administrator credential, and sessions.
Legacy SQLite settings columns remain unused; startup and Settings never read or
update them, including the old configuration documents and projection journal.

Web Settings validates policy and prebinds a changed listener, then writes a
private temporary TOML file, fsyncs it, atomically replaces `controller.toml`, and
fsyncs `data`. Only after persistence succeeds does it update runtime policy and
hot-apply scheduler, independent transfer limits, auth, timeouts, retry, and the
listener. Failed persistence leaves runtime unchanged. Stale Settings generations
return conflict; the generation is in memory and resets on restart. A shared
admission lock holds pause/config commits behind already admitted submissions
and prevents new reservations, uploads, or submissions after pause commits.
Processing and downstream work continue. Shutdown pauses admission in memory only,
so it preserves manual TOML edits and does not persist an implicit operator pause.

Manual TOML edits require Controller restart. There is no automatic TOML file
watching, polling, or database reconciliation. A crash after the TOML replacement
naturally loads the saved configuration on restart. `--host` and `--port` overrides
persist directly to TOML at startup and remain effective on later starts.

Same-origin proof compares parsed Host and Origin authorities, including ports
and HTTP/HTTPS default ports. With `secure_cookie=false`, same-host HTTP and HTTPS
are accepted. With `secure_cookie=true`, only same-host HTTPS is accepted.
Malformed or foreign origins and missing required proof are rejected. Forwarded
headers are not trusted. HTTPS reverse-proxy first-access setup works with defaults;
an HTTPS session can enable Secure cookies in Settings, then subsequent mutations
require HTTPS Origin plus CSRF proof. As before, changing Secure policy invalidates
old sessions; sign in again to receive a Secure cookie.

Keep the default loopback listener unless another network layer requires remote
access. `secure_cookie = false` allows both HTTP and HTTPS. Use
same-origin HTTPS and `secure_cookie = true` when browsers can reach Controller
through an untrusted network. Browser sessions use an HttpOnly,
SameSite=Strict cookie plus CSRF proof. Never put passwords, Authorization
headers, cookies, or CSRF values in URLs, configuration, logs, or source control.

After setup, `POST /api/auth/login` accepts the administrator password. API
clients may use the same password as a Bearer credential, but should read it
through protected interactive input rather than a command argument or reusable
script.

## Data and Backup

SQLite is authoritative for operational state; TOML owns configuration.
Stop Controller cleanly before a simple filesystem
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
matching media at its recorded absolute paths, and matching worker data. Start
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

The image defaults to working directory `/workspace` and startup arguments
`--host 0.0.0.0`, so neither `--workdir` nor an explicit `--host` is needed.
It runs as UID/GID `10001:10001` unless overridden. The `--user` option below
maps file ownership to your host user.

Mount private Controller data and media separately using `-v`. This example uses
`$HOME/Videos` as the host media directory; replace it with your existing media
directory:

```bash
mkdir -p "$PWD/data"
docker run -d --name videnoa-controller \
  --user "$(id -u):$(id -g)" \
  -p 127.0.0.1:3001:3001 \
  -v "$PWD/data:/workspace/data" \
  -v "$HOME/Videos:/media" \
  controlnet/videnoa-controller:latest
```

Open `http://localhost:3001` for first-access setup. Configuration and database
files persist in `./data/controller.toml` and `./data/controller.sqlite3` on the
host. `/workspace/data` is private and forbidden for task input/output. Use task
paths such as `/media/input.mkv` and `/media/output.mp4`.

Separate data and media bind mounts are supported. Their final rename can return
`EXDEV` even on the same host disk, activating the verified copy-and-delete
fallback described above. The final filename is visible during that copy;
Jellyfin may scan it before completion.

Keep the published host port on loopback for trusted first setup, or protect it
with firewalling and a same-origin HTTPS reverse proxy. Do not expose an
uninitialized setup endpoint to an untrusted network.

Task paths inside Docker are container-visible paths. No Docker path translation
exists and media need not live under `/workspace`. If another application sends
`/mnt/user/media/anime/E08.mkv`, mount media at that same container path instead
of `/media`.

To retain atomic publication without copy fallback, a common bind mount can keep
workspace and media in separate directories on the same mounted filesystem.
For example, when your media is under `/mnt/user/media`:

```bash
mkdir -p /mnt/user/controller-workspace
docker run -d --name videnoa-controller \
  --user "$(id -u):$(id -g)" \
  -p 127.0.0.1:3001:3001 \
  -v /mnt/user:/mnt/user \
  --workdir /mnt/user/controller-workspace \
  controlnet/videnoa-controller:latest
```

Only this alternative overrides the default working directory; private state
then lives under `/mnt/user/controller-workspace/data`. Atomic rename is preferred;
cross-mount outputs use the copy fallback when necessary.
Changing the Controller listener port in Docker also requires updating container
port mapping, reverse proxy, and health-check configuration. Listener settings
remain available normally.

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
- Setup returns `403`: send a valid same-host `Origin`; Secure cookies require HTTPS.
- Setup returns `400`: use matching password fields with at least 12 bytes.
- Setup returns `409`: the administrator credential is already initialized; use
  login rather than trying to replace it through setup.
- Health works but readiness fails: finish administrator setup, authenticate,
  and inspect readiness before admitting tasks.
- A path is rejected: use process-accessible task media outside `./data`,
  remove traversal and symlink components, and preserve existing destinations.
- A worker stays offline or incompatible: verify its credential-free HTTP(S)
  URL, Videnoa health, persistent data, and exact workflow interface.
- Output already exists: preserve it and create a new task with another path.
