# Videnoa Controller Operations Guide

This guide covers installation, API use, security, recovery, and distribution
for `videnoa-controller`. Archive users receive the self-contained
[`README-controller.md`](../README-controller.md).

## Architecture and Boundaries

Controller is a GPU-free coordination service with a private working location. It serves the Web UI, validates
task paths, stores task and attempt history, schedules remote work, streams
transfers, and reconciles restarts.

SQLite is authoritative for operational state: the administrator credential, sessions,
tasks, attempts, assignments, idempotency, retry state, and recovery. Runtime
channels and SSE only prompt work or client refetches. They are not a second
queue or history store.

The existing `videnoa` service remains the GPU application. A Controller worker
record names one remote Videnoa HTTP(S) service and its compute capacity.
Controller does not import `videnoa-core`, load models, select GPUs, manage
network mounts, or require CUDA, cuDNN, TensorRT, or ONNX Runtime.

Controller intake is manual Web UI creation or authenticated `POST /api/tasks`.
External automation such as ANI-RSS may call that generic endpoint after making
its own media decision. Controller has no watcher, directory polling,
qBittorrent integration, cron discovery, or rules engine.

## Install and Configure

Run the binary from the directory that will be its workspace:

```bash
./videnoa-controller
```

Windows PowerShell:

```powershell
.\videnoa-controller.exe
```

No root access, system directory, copied configuration, media-root preparation,
or credential file is required. The default listener is `127.0.0.1:3001`.
Controller creates only `./data/controller.toml` and
`./data/controller.sqlite3` on first start. SQLite sidecars and transient
per-task UUID directories may also appear under `./data`.

The optional process overrides are:

```text
videnoa-controller [--host <IP>] [--port <PORT>]
videnoa-controller --help
videnoa-controller --version
```

Use [`controller.example.toml`](../controller.example.toml) as a field reference,
not as a required installation step. Raw TOML accepts exactly these sections:

| Section | Fields |
|---|---|
| `server` | `host`, `port` |
| `auth` | `secure_cookie`, `session_absolute_seconds`, `session_idle_seconds` |
| `scheduler` | `paused`, `default_compute_slots`, `prefetch_per_worker`, `max_concurrent_uploads`, `max_concurrent_downloads` |
| `timeouts` | `health_seconds`, `poll_seconds`, `transfer_seconds` |
| `retry` | `initial_seconds`, `maximum_seconds`, `max_attempts` |

Unknown fields are rejected. Defaults are loopback port 3001, non-Secure
cookies, 24-hour absolute sessions, one-hour idle sessions, one compute slot,
one prefetched task, one upload, one download, health/poll/transfer timeouts of
10/5/300 seconds, retry delays of 1 through 60 seconds, and five attempts.

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

## First Administrator Setup

Open `http://127.0.0.1:3001/` after first start. The Web UI asks for the first
administrator password and confirmation. The password must contain at least 12
bytes. Controller stores only its Argon2id hash in SQLite.

`GET /api/auth/setup` returns an `initialized` boolean. `POST /api/auth/setup`
accepts this exact shape:

```json
{
  "password": "entered interactively",
  "password_confirmation": "entered interactively"
}
```

The POST requires valid same-host Origin proof under the transport policy below. A
successful setup returns the existing `LoginResponse`, sets the session cookie,
and returns `x-csrf-token`. A mismatched or shorter password returns 400, an
origin mismatch returns 403, and an already initialized setup returns 409.

Do not expose first setup to an untrusted network. Complete it through loopback,
your trusted LAN over HTTP, or a protected same-origin HTTPS endpoint.

## Security

Plain HTTP on a trusted LAN is a supported deployment, including a LAN IP
address or hostname. Setup, login, task operations, Workers, Settings, and live
updates work with the default `secure_cookie=false`. HTTPS and public Internet
exposure are not prerequisites. Leave **Require secure session cookie** off for
HTTP deployments; enabling it explicitly requires HTTPS for session use.

Browser sessions use an HttpOnly, SameSite=Strict cookie and CSRF proof.
`Secure` is present when `secure_cookie = true`. Cookie-authenticated mutations
require both `x-csrf-token` and a valid same-origin `Origin`. Bearer requests are
CSRF exempt. Controller does not enable permissive CORS.

Same-origin proof compares parsed Host and Origin authorities, including ports
and HTTP/HTTPS default ports. With `secure_cookie=false`, same-host HTTP and HTTPS
are accepted. With `secure_cookie=true`, only same-host HTTPS is accepted.
Malformed or foreign origins and missing required proof are rejected. Forwarded
headers are not trusted. HTTPS reverse-proxy first-access setup works with defaults;
an HTTPS session can enable Secure cookies in Settings, then subsequent mutations
require HTTPS Origin plus CSRF proof. As before, changing Secure policy invalidates
old sessions; sign in again to receive a Secure cookie.

`POST /api/auth/login` accepts `{"password":"..."}` after setup.
`GET /api/auth/session` returns current session metadata and rotates the
cookie-session CSRF proof. `POST /api/auth/logout` revokes a cookie session and
expires its cookie.

Keep the listener on loopback unless remote access is required. Trusted LAN
browsers can connect directly over HTTP. To add transport encryption, use a
same-origin HTTPS reverse proxy and enable `secure_cookie`; HTTPS does not
require exposing the service publicly. Never
record passwords, Authorization values, cookies, CSRF values, or setup bodies in
logs, configuration, URLs, or source control.

## Workspace and Paths

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

Input must be an existing regular file with an extension. Output is an exact,
caller-selected missing leaf with an extension. Parent traversal, symlink
components, changed file identity/content, non-regular input, and existing or
racing output fail closed. Controller never overwrites or auto-renames output.

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

## Register Workers and Workflows

Register workers in the Web UI or through `POST /api/workers`:

```json
{
  "name": "gpu-host-1",
  "api_url": "https://gpu-host-1.example.internal:3000/",
  "enabled": true,
  "compute_slots": 1
}
```

The reserved domain is illustrative. Worker URLs are credential-free HTTP(S)
base URLs without query strings or fragments. Controller combines each worker's
workflow and preset catalogs. A name is eligible only when its interface has
`Path` inputs named exactly `input` and `output`. Controller does not deploy or
synchronize workflows.

Worker updates use `PUT /api/workers/{id}` with the current version and all
mutable fields. Enable and disable use `POST /api/workers/{id}/enable` and
`/disable`. Delete uses `DELETE /api/workers/{id}?version=N` and succeeds only
for an unreferenced current record.

Every worker must persist its Videnoa data. `jobs.db` stores keyed job identity,
and worker task workspaces remain recovery evidence until cleanup. Losing this
data can produce `remote_state_ambiguous`; Controller never guesses that compute
is safe to repeat.

## Scheduling and Lifecycle

Queued tasks are ordered by priority descending, creation ascending, then ID.
Eligible workers are enabled, online, workflow-compatible, and within capacity.

Compute capacity is occupied by `submitting` and `processing`. Stage-in capacity
is occupied by `reserved`, `uploading`, and `staged`. Downstream transfer,
verification, publication, and cleanup consume neither. Scheduler pause is
durable and blocks new reservation, stage-in admission, and compute submission
while allowing downstream work to converge.

Task statuses are `queued`, `reserved`, `uploading`, `staged`, `submitting`,
`processing`, `remote_completed`, `downloading`, `verifying`, `publishing`,
`remote_cleanup`, `completed`, `failed`, and `cancelled`.

Each compute attempt has a durable submission key before remote `POST /api/run`.
The same key and body returns the existing remote job; a changed body conflicts.
Transfer and cleanup failures use bounded persisted retry. Downstream retries do
not repeat successful AI work.

Cancellation is accepted from queued through verifying. Publishing, remote
cleanup, and terminal tasks reject cancellation. Ambiguous failures are not
retryable.

## No-Clobber and Ambiguity

Task intake requires the exact output leaf not to exist. Before publication,
Controller rechecks the output capability and verified artifact, then attempts
atomic no-replace rename. Only `EXDEV` activates copy fallback, with exclusive
creation of the final filename. Existing or racing output is never overwritten
or auto-renamed. A partial output is resumed only with matching private ownership
evidence and a byte-for-byte match against the verified source prefix.

If recovery cannot prove whether remote compute or local publication completed,
Controller records `remote_state_ambiguous` or `publication_ambiguous` and
preserves evidence.

For `remote_state_ambiguous`, disable the worker and preserve Controller data,
the worker's `jobs.db`, workspace, logs, IDs, and exact workflow parameters. Do
not retry or submit equivalent work until identity is proven.

For `publication_ambiguous`, pause scheduling and preserve the final path and
verified transient artifact. Compare regular-file type, length, and SHA-256 with
durable evidence. Do not delete, rename, overwrite, or force retry either path.

## API Reference

All JSON DTOs reject unknown fields. Health and setup status are public. Setup is
available only before initialization. Readiness, tasks, workers, settings,
counts, SSE, and logout require session or Bearer authentication after setup.

### Health and Authentication

| Method | Route | Result |
|---|---|---|
| `GET` | `/api/health` | `200 {"status":"ok"}` |
| `GET` | `/api/readiness` | Authenticated readiness checks |
| `GET` | `/api/auth/setup` | `{"initialized":bool}` |
| `POST` | `/api/auth/setup` | First credential plus login response; valid Origin required |
| `POST` | `/api/auth/login` | Session cookie, CSRF header, and session JSON |
| `GET` | `/api/auth/session` | Current session metadata |
| `POST` | `/api/auth/logout` | `{"logged_out":true}` |

### Create and Read Tasks

`POST /api/tasks` requires one `Idempotency-Key` header containing 1 to 255
visible ASCII bytes. Request fields are:

| Field | Rule |
|---|---|
| `input_path` | Existing regular process-visible media file, outside private `data` |
| `output_path` | Missing process-visible output leaf, outside private `data` |
| `workflow` | Non-empty UTF-8 name, maximum 128 bytes |
| `priority` | Integer from -100 through 100 |
| `source` | `manual` or `api` |
| `source_reference` | String up to 512 bytes or `null` |

First creation returns 201. Replaying the same key and canonical body returns the
original task with 200. The same key with a different body returns 409.

`GET /api/tasks` supports `limit`, `offset`, `status`, `worker_id`, `workflow`,
`source`, `failure_stage`, `search`, `sort`, and `direction`. `GET
/api/tasks/{id}` returns the task plus a paginated attempts array.

`POST /api/tasks/{id}/cancel` and `POST /api/tasks/{id}/retry` require the
current `version`. `GET /api/status-counts` returns all lifecycle categories,
including zero counts.

### Workers and Settings

| Method | Route | Request or result |
|---|---|---|
| `GET` | `/api/workers` | Worker list, capabilities, and capacity |
| `POST` | `/api/workers` | Create `name`, `api_url`, `enabled`, `compute_slots` |
| `PUT` | `/api/workers/{id}` | Current version plus all mutable fields |
| `GET` | `/api/settings` | Version, path metadata, server, auth policy, scheduler, timeouts, retry |
| `PUT` | `/api/settings` | Current `version` plus complete `server`, `auth`, `scheduler`, `timeouts`, `retry` |
| `POST` | `/api/scheduler/pause` | `{"version":N}` |
| `POST` | `/api/scheduler/resume` | `{"version":N}` |

Settings path metadata is read-only and contains `workspace`, `data_root`, and
`config_file`. The response exposes server plus scalar auth policy. The update
groups auth policy under `auth`. Every mutable field is persisted and
hot-applied, including listener and authentication policy. A listener update is
rejected before persistence when the requested address cannot be bound.

### SSE Semantics

`GET /api/events` is authenticated SSE. Every connection first receives a
`refetch` event with reason `snapshot_required`. Durable changes may emit
`task_updated`, `worker_updated`, or `scheduler_updated`; lag and deletion use
`refetch`. SSE is an invalidation hint, not durable history.

### Errors

Operational errors use an error envelope with stable code, message, retryable
flag, and field errors. Stable codes include `invalid_request`, `unauthorized`,
`forbidden`, `not_found`, `conflict`, `unavailable`, `internal_error`,
`remote_state_ambiguous`, and `publication_ambiguous`. Authentication endpoints
use the smaller top-level `error` form.

## Backup and Restore

For a source-accurate filesystem backup:

1. Pause scheduling through Web UI Settings or `/api/scheduler/pause`.
2. Let work reach known states and stop Controller cleanly.
3. Copy the complete workspace `data` directory, including SQLite sidecars and
   transient task directories.
4. Preserve task media and every worker's persistent Videnoa data, especially
   `jobs.db` and worker workspaces.
5. Record Controller and worker versions without recording credentials.

Restore only while Controller and affected workers are stopped. Restore the
matching Controller data and worker data, and media at its recorded absolute
paths. Start Controller, check health and authenticated readiness, inspect every
nonterminal task and worker, then resume scheduling.

## Upgrade and Rollback

Before an upgrade, pause, drain to known states, stop, and take the complete
backup above. Replace only the executable or image and preserve the workspace.
Startup applies pending migrations atomically. Verify `/api/health`,
`/api/readiness`, Web login, workers, retained tasks, and recovery before resume.

Rollback requires the pre-upgrade Controller and worker snapshots. Never point
an older binary at a database already migrated by a newer version. There is no
manual migration command or supported migration downgrade.

## Troubleshooting

```bash
curl --fail http://127.0.0.1:3001/api/health
```

- Startup cannot create `data`: verify workspace permissions for the process or
  container user.
- Setup 400: password and `password_confirmation` must match and contain at least
  12 bytes.
- Setup 403: `Origin` must match Host; Secure cookies require HTTPS.
- Setup 409: setup already completed; use login.
- `401`: credential is missing, expired, revoked, or invalid.
- `403` mutation: valid Origin or current CSRF proof is missing.
- Path rejection: use accessible media outside private `data`; remove
  traversal and symlink components.
- Worker offline: verify network/TLS, Videnoa health, persistent data, and
  workflow compatibility.
- Output exists: preserve it and create a task with a different output path.
- Ambiguous state: follow the evidence-preservation procedures above.

## Distribution and Release

Images are:

```text
controlnet/videnoa-controller:<version>
controlnet/videnoa-controller:latest
```

The image uses Debian bookworm slim, embeds the frontend, exposes port 3001,
and uses `videnoa-controller` as entrypoint. It declares no named volumes.

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

Archives are:

```text
videnoa-controller-v<version>-linux-x86_64.tar.gz
videnoa-controller-v<version>-windows-x86_64.zip
```

Each archive root contains only `LICENSE`, `README-controller.md`,
`controller.example.toml`, and the platform executable. Frontend assets are
embedded. Linux packaging uses `scripts/package_controller.sh`; native Windows
packaging uses `scripts/package_controller.ps1`.
