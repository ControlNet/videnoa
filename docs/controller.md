# Videnoa Controller Operations Guide

This guide covers installation, API use, security, recovery, and release
operations for `videnoa-controller`. Archive users also receive
[`README-controller.md`](../README-controller.md), which contains a complete
first-run and critical recovery path without this repository file.

## Architecture and Boundaries

Controller is a GPU-free coordination service normally placed on the NAS that
owns the media paths. It serves the Web UI, validates NAS path capabilities,
stores task and attempt history, schedules remote work, streams transfers, and
reconciles restarts. Its durable database is
`data_root/controller.sqlite3`.

The NAS provides the configured input and output roots. `input_path` is an
existing regular file under an input root and is never modified. `output_path`
is an exact caller-selected, non-existing path under an output root. Input and
output extensions can differ. Downloaded bytes first enter Controller-owned
`temp_root`; final publication is one atomic no-replace rename into the exact
output path. Controller never writes an intermediate file under an output root.

The existing `videnoa` service remains the GPU application. A Controller worker
record names one remote Videnoa HTTP(S) service and its compute slot capacity.
Videnoa owns model execution, its own `jobs.db`, and task workspaces. Controller
doesn't import `videnoa-core`, load models, select GPU IDs, schedule VRAM, or
require CUDA, cuDNN, TensorRT, or ONNX Runtime.

SQLite is authoritative for tasks, attempts, assignments, settings, sessions,
idempotency, retry state, and recovery. Runtime channels and SSE only wake work
or prompt clients to refetch. They are not a second queue or history store.

Controller has exactly two intake modes: manual Web UI creation and external
HTTP `POST /api/tasks`. ANI-RSS can call that generic POST after its own media
decision. Controller has no dedicated ANI-RSS adapter, watcher, directory
polling, qBittorrent integration, cron discovery, or rules engine.

Other non-goals include workflow deployment or synchronization, direct browser
calls to Videnoa, SSH/SFTP/rsync, object storage, network-mount management,
resumable transfer, brokers, Kubernetes, multiple users, roles, ACLs, media
browsing, automatic history deletion, overwrite fallback, and blind compute
resubmission.

## Install and Configure

The CLI has one optional subcommand and three global options:

```text
videnoa-controller [--config <PATH>] [--host <IP>] [--port <PORT>]
videnoa-controller hash-password
videnoa-controller --help
videnoa-controller --version
```

`--host` and `--port` override loaded configuration. Without `--config`, typed
defaults are used, but all default paths and the default password hash file must
already exist. An explicit config is clearer for operations.

Start from [`controller.example.toml`](../controller.example.toml). Unknown TOML
keys are rejected. Every configured root and data/temp directory must exist and
must be a non-symlink directory. At least one input and output root is required.
`temp_root` must not equal, contain, or be contained by an output root, and it
must be on the same filesystem as every output root so publication can remain
atomic. A nested mount discovered only at publication is rejected without a
copy fallback.
The password hash path must name a readable regular, non-symlink file containing
an Argon2id PHC string.

Configuration is layered as defaults, exact TOML file, then environment
overrides using `VIDENOA_CONTROLLER_` and `__` between nesting levels. For
example, `VIDENOA_CONTROLLER_SERVER__PORT=3101` overrides `server.port`.
Environment overrides are suitable for non-secret values and paths. The admin
password itself is never a config or environment value. Inject only the hash
file through a protected mounted file.

Linux setup:

```bash
sudo install -d -m 0750 /etc/videnoa-controller /var/lib/videnoa-controller /var/tmp/videnoa-controller
sudo install -d -m 0750 /srv/media/incoming /srv/media/library
sudo install -m 0640 controller.example.toml /etc/videnoa-controller/controller.toml
sudo sh -c './videnoa-controller hash-password > /var/lib/videnoa-controller/admin-password.phc'
sudo chmod 0600 /var/lib/videnoa-controller/admin-password.phc
./videnoa-controller --config /etc/videnoa-controller/controller.toml
```

Run hash generation from an interactive terminal. It reads hidden input when
stdin is a terminal. Don't pass the password as a command argument. Review file
ownership before running Controller as a service account.

Windows PowerShell setup:

```powershell
New-Item -ItemType Directory -Force C:\VidenoaController\Data, C:\VidenoaController\Temp, D:\Media\Incoming, D:\Media\Library | Out-Null
Copy-Item .\controller.example.toml C:\VidenoaController\controller.toml
.\videnoa-controller.exe hash-password | Set-Content -NoNewline C:\VidenoaController\admin-password.phc
.\videnoa-controller.exe --config C:\VidenoaController\controller.toml
```

Edit the copied TOML to use quoted Windows paths such as
`"D:\\Media\\Incoming"`. Windows ACLs, not Unix modes, protect the config, data,
temp, and hash file.

The config sections are:

| Section | Fields | Rules |
|---|---|---|
| `server` | `host`, `port` | IP address and port 1 through 65535 |
| `paths` | `input_roots`, `output_roots`, `data_root`, `temp_root` | Existing non-symlink directories; temp is disjoint from and on the same filesystem as every output root |
| `auth` | `password_hash_file`, `secure_cookie`, `session_absolute_seconds`, `session_idle_seconds` | Positive lifetimes; idle must not exceed absolute |
| `scheduler` | `paused`, `default_compute_slots`, `prefetch_per_worker`, `max_concurrent_uploads`, `max_concurrent_downloads` | Slots and concurrency are positive; prefetch may be zero |
| `timeouts` | `health_seconds`, `poll_seconds`, `transfer_seconds` | Positive seconds |
| `retry` | `initial_seconds`, `maximum_seconds`, `max_attempts` | Positive; initial must not exceed maximum |

Defaults are `127.0.0.1:3001`, 24-hour absolute sessions, one-hour idle
sessions, one compute slot, one prefetched task per worker, one global upload,
one global download, health 10 seconds, poll 5 seconds, transfer 300 seconds,
retry 1 to 60 seconds, and five attempts.

## Security

### Password and Rotation

`videnoa-controller hash-password` emits one Argon2id PHC string. Store it in a
separate protected file. TOML holds only `password_hash_file`. Don't store a
password, Bearer value, cookie, CSRF proof, or PHC text in source control,
documentation, logs, shell history, process arguments, or browser storage.

To rotate credentials safely:

1. Generate a new hash to a new protected file through interactive input.
2. Set ownership and permissions for the Controller service account.
3. Atomically replace the configured hash file.
4. Confirm a fresh login or Bearer request, then remove the old protected file.

Controller reloads the hash for every login, Bearer check, cookie-session check,
and readiness check. Rotation invalidates existing sessions through the stored
hash fingerprint and rejects the old password without restart.

### Browser Sessions and API Bearer

`POST /api/auth/login` accepts `{"password":"..."}` and sets
`videnoa_session` with `HttpOnly`, `SameSite=Strict`, `Path=/`, and an absolute
`Max-Age`. `Secure` is present when `secure_cookie = true`. The response also
returns `x-csrf-token`; the Web UI keeps this proof in memory.

`GET /api/auth/session` accepts a valid cookie or Bearer header. Cookie access
rotates the CSRF proof and returns it in `x-csrf-token`. `POST
/api/auth/logout` revokes a cookie session and expires the cookie. Bearer logout
returns success but has no durable Bearer token to revoke.

Cookie-authenticated mutations require both `x-csrf-token` and an exact Origin
matching the request scheme and Host. The expected scheme is `https` when
secure cookies are enabled and `http` otherwise. Bearer requests are CSRF
exempt. Controller doesn't enable permissive CORS.

Failed login is tracked per source IP in memory. The sixth failure inside five
minutes returns `429 {"error":"rate_limited"}`. Successful login clears that
IP's failures. Auth responses never reflect credentials.

For scripts, keep the raw password out of command arguments. One local pattern
is a mode-0600 temporary curl header file populated from hidden terminal input:

```bash
umask 077
AUTH_HEADER="$(mktemp)"
read -rsp 'Controller password: ' CONTROLLER_PASSWORD; printf '\n'
printf 'Authorization: Bearer %s\n' "$CONTROLLER_PASSWORD" > "$AUTH_HEADER"
unset CONTROLLER_PASSWORD
curl --fail --header @"$AUTH_HEADER" http://127.0.0.1:3001/api/readiness
rm -f "$AUTH_HEADER"
```

### Network and Paths

Keep the listener on loopback unless another network layer needs it. Tailscale
is suitable for private reachability, but private routing doesn't replace HTTPS
for browser cookie confidentiality. Put Controller behind a trusted same-origin
HTTPS reverse proxy and keep `secure_cookie = true`. Use the HTTP override only
on an explicitly trusted network.

Authentication never expands filesystem rights. Input and output paths must
remain inside configured root capabilities. Symlink roots/components,
traversal, changed root identities, non-regular inputs, existing outputs, and
changed input identity/content fail closed. Worker URLs allow only
credential-free HTTP(S) base URLs without query strings or fragments.

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

The example name and reserved domain are descriptive only. Don't paste a real
credential into the URL. Creation returns `201` and an initially offline worker.
Health refresh populates online state and compatible capabilities.

Controller merges the Videnoa `/api/workflows` and `/api/presets` catalogs. A
name is eligible only when its interface has `Path` inputs named exactly
`input` and `output`. Controller doesn't deploy workflows, so every intended
worker must already provide a compatible workflow or preset with the same name.

Worker updates use `PUT /api/workers/{id}` with the current `version` plus
`name`, `api_url`, `enabled`, and `compute_slots`. Enable and disable use
`POST /api/workers/{id}/enable` and `/disable` with `{"version":N}`. Delete
uses `DELETE /api/workers/{id}?version=N`. Duplicate names or normalized URLs,
stale versions, capacity below durable usage, and deletion of a worker referenced
by any task return `409`. Disable a referenced worker instead of deleting it.

Every Videnoa worker must persist its data directory. `jobs.db` stores keyed job
identity, and `workspace/<task-id>/` holds remote task files until cleanup.
Losing or replacing that data removes reconciliation evidence. Controller then
records `remote_state_ambiguous` and never guesses that compute can be repeated.

## Scheduling and Lifecycle

Queued tasks are selected by `priority DESC, created_at ASC, id ASC`. Eligible
workers are enabled, online, compatible, and have stage-in budget. Selection
favors lower compute occupancy, then lower stage-in occupancy, older assignment
time, and worker ID.

| Resource | Occupying states | Limit |
|---|---|---|
| Compute | `submitting`, `processing` | `compute_slots` |
| Stage-in | `reserved`, `uploading`, `staged` | Unfilled compute demand plus `prefetch_per_worker` |
| Stage-out | `remote_completed`, `downloading`, `verifying`, `publishing`, `remote_cleanup` | Neither compute nor stage-in slots |

Reservation is admitted only when stage-in occupancy is below
`max(compute_slots - compute_occupancy, 0) + prefetch_per_worker`. The later
`staged -> submitting` transition separately claims compute in a SQLite-atomic
update that checks both current compute occupancy and persisted pause. Concurrent
staged tasks cannot claim the same final free compute slot.

Thus `compute_slots=1, prefetch_per_worker=1` allows one downloading task, one
processing task, and one uploading or staged task on the same worker. Controller
still permits at most one active upload per worker and uses independent global
upload and download limits. Uploads feeding unfilled compute demand outrank
optional prefetch. Worker `used_slots` counts only submitting and processing;
`assigned_tasks` counts every assigned nonterminal task. Reducing compute slots
is rejected only when it would be below actual compute occupancy.

Scheduler pause is durable. `POST /api/scheduler/pause` with the current settings
version blocks new reservation, prefetch/upload admission, and compute submission.
It preserves staged reservations and allows polling, downloads, verification,
publication, and cleanup to converge. Resume uses `/api/scheduler/resume` with
the current version.

Task statuses are `queued`, `reserved`, `uploading`, `staged`, `submitting`,
`processing`, `remote_completed`, `downloading`, `verifying`, `publishing`,
`remote_cleanup`, `completed`, `failed`, and `cancelled`.

Each compute attempt has a durable submission key before Controller calls remote
Videnoa `POST /api/run`. Same key and body returns the existing job. A changed
body conflicts. The remote workspace is `<task-id>/input.<input-ext>` and
`<task-id>/output.<output-ext>`. Remote paths are opaque Videnoa workflow values,
not Controller-local filesystem paths.

Transient transfer, health, and cleanup failures use persisted bounded retry.
An explicit `processing_failed` retry verifies the remote job is terminal and
the old workspace is cleaned, then creates a new attempt, submission key, and
possibly another worker. Transfer, verification, publication, and cleanup
retries resume the failed downstream stage on the same attempt. Successful AI
work is never repeated for a downstream failure.

Cancellation is accepted from queued through verifying and records intent before
side effects. Submitting must reconcile acceptance before remote cancellation.
Publishing, remote cleanup, and terminal tasks return `409` because irreversible
publication or cleanup must converge.

## No-Clobber and Ambiguity

Task intake requires the exact output leaf not to exist. Before publication,
Controller rechecks the output capability, re-hashes the verified file under
`temp_root`, and atomically renames it directly to the exact final path with
platform no-replace semantics. Existing or racing output is never overwritten,
auto-renamed, or used as a reason to copy. The source and destination directory
entries are durably synced before lifecycle completion. To use a different
destination, create a new task.

On publication recovery, a verified temp file with no final retries the same
direct rename. A final with no verified temp file converges only when its length
and SHA-256 match durable evidence. Mismatch, non-regular files, or simultaneous
temp and final files become `publication_ambiguous`; Controller preserves the
unknown final and does not overwrite either path. A non-null destination staging
name persisted by an older Controller is inspected conservatively: if that
legacy file is absent, direct temp/final recovery proceeds; if it exists or
cannot be safely inspected, recovery records `publication_ambiguous` and leaves
the legacy artifact untouched.

Operator response to `remote_state_ambiguous`:

1. Disable the worker and preserve Controller `controller.sqlite3`, Videnoa
   `jobs.db`, workspace data, logs, task/attempt IDs, remote job ID, and params.
2. Compare identity evidence. Don't delete a job/workspace or retry across the
   ambiguity. Don't submit an equivalent replacement task.
3. Restore missing evidence from a known matching backup when possible. If proof
   can't be recovered, retain the failed record and resolve the media outcome
   outside automated Controller retry.

Operator response to `publication_ambiguous`:

1. Pause scheduling and preserve the final path, verified temp artifact, and any
   legacy hidden staging artifact named by durable evidence.
2. Compare regular-file type, length, and SHA-256 with durable evidence.
3. Don't delete, rename, overwrite, or force retry either artifact. Preserve the
   task record for audit and perform any manual media decision outside Controller.

## API Reference

All JSON DTOs reject unknown fields. Public liveness and login are the only
unauthenticated useful routes. Readiness, tasks, workers, settings, status counts,
SSE, and logout require session or Bearer authentication. Cookie mutations also
require Origin and CSRF proof.

### Health and Authentication

| Method | Route | Result |
|---|---|---|
| `GET` | `/api/health` | `200 {"status":"ok"}` without authentication |
| `GET` | `/api/readiness` | `200 ready` or `503 not_ready`; checks `migrations`, `authentication`, `root_handles` |
| `POST` | `/api/auth/login` | Session cookie, CSRF response header, session JSON |
| `GET` | `/api/auth/session` | Current session metadata; rotates cookie-session CSRF |
| `POST` | `/api/auth/logout` | `{"logged_out":true}` and expired session cookie |

### Create and Read Tasks

`POST /api/tasks` requires exactly one `Idempotency-Key` header containing 1 to
255 visible ASCII bytes. Request fields are exact:

| Field | Type and rule |
|---|---|
| `input_path` | Non-empty existing regular file under an input root, with extension |
| `output_path` | Non-empty missing leaf under an output root, with extension |
| `workflow` | Non-empty UTF-8 string, maximum 128 bytes |
| `priority` | Integer from -100 through 100 |
| `source` | `manual` or `api`, informational only |
| `source_reference` | String up to 512 bytes or `null`, informational only |

Generic automation request, including ANI-RSS calling Controller after its own
decision:

```bash
umask 077
AUTH_HEADER="$(mktemp)"
read -rsp 'Controller password: ' CONTROLLER_PASSWORD; printf '\n'
printf 'Authorization: Bearer %s\n' "$CONTROLLER_PASSWORD" > "$AUTH_HEADER"
unset CONTROLLER_PASSWORD
IDEMPOTENCY_KEY="$(cat /proc/sys/kernel/random/uuid)"
curl --fail-with-body \
  --header @"$AUTH_HEADER" \
  --header 'Content-Type: application/json' \
  --header "Idempotency-Key: $IDEMPOTENCY_KEY" \
  --data '{"input_path":"/srv/media/incoming/title.mkv","output_path":"/srv/media/library/title.mp4","workflow":"anime-2x","priority":0,"source":"api","source_reference":null}' \
  http://127.0.0.1:3001/api/tasks
rm -f "$AUTH_HEADER"
```

The media paths and workflow above are illustrative operator inputs, not bundled
test credentials. On a connection loss, replay the exact body with the same key.
First creation returns `201`; same key and canonical body returns the original
task with `200`; same key and different body returns `409`.

`GET /api/tasks` returns `items`, `total`, `limit`, and `offset`. Query fields:

| Query | Values |
|---|---|
| `limit` | Default 100, minimum 1, maximum 500 |
| `offset` | Nonnegative integer, default 0 |
| `status` | One exact task status |
| `worker_id` | Worker UUID |
| `workflow` | Exact workflow name |
| `source` | `manual` or `api` |
| `failure_stage` | `reservation`, `upload`, `submission`, `processing`, `download`, `verification`, `publication`, `local_cleanup`, or `remote_cleanup` |
| `search` | Case-insensitive substring across input and output paths |
| `sort` | `priority`, `created_at`, `completed_at`, `status`, `worker`, or `duration` |
| `direction` | `asc` or `desc` |

Default ordering is priority descending, creation ascending, then ID ascending.
Other sorts use ID as a stable tie-breaker, and nullable history sorts place
unknown values last.

`GET /api/tasks/{id}?limit=100&offset=0` returns `task`, a server-paginated
`attempts` array, `total`, `limit`, and `offset`. Task output includes identity,
version, status, exact paths/extensions/workflow/source, input size, worker and
remote job IDs, progress, attempt count, failure, cancellation time, and task
timestamps. Attempt output includes its number, submission key, worker/job/remote
paths, progress, retry metadata, failure, and timestamps.

### Task Actions and Counts

`POST /api/tasks/{id}/cancel` and `POST /api/tasks/{id}/retry` require
`{"version":N}` from the latest task representation. Cancel returns `task_id`,
current `status`, and `cancel_requested_at`. Retry returns `task_id`, the resumed
or new `attempt_id`, and current `status`. Stale versions and disallowed actions
return `409`.

`GET /api/status-counts` returns all 14 task statuses in lifecycle order with a
count, including zero categories, plus `total`.

### Workers and Settings

| Method | Route | Request or result |
|---|---|---|
| `GET` | `/api/workers` | `items` and `total`; each worker includes capabilities and capacity |
| `POST` | `/api/workers` | `name`, `api_url`, `enabled`, `compute_slots`; returns `201` |
| `PUT` | `/api/workers/{id}` | Current `version` plus all mutable worker fields |
| `POST` | `/api/workers/{id}/enable` | `{"version":N}` |
| `POST` | `/api/workers/{id}/disable` | `{"version":N}` |
| `DELETE` | `/api/workers/{id}?version=N` | Deletes only an unreferenced current record |
| `GET` | `/api/settings` | Versioned runtime settings plus read-only paths/session policy |
| `PUT` | `/api/settings` | Current `version`, complete `scheduler`, `timeouts`, and `retry` objects |
| `POST` | `/api/scheduler/pause` | `{"version":N}` |
| `POST` | `/api/scheduler/resume` | `{"version":N}` |

Runtime settings updates allow timeout values up to seven days and retry
`max_attempts` up to 100. Root, data, temp, hash-file, secure-cookie, and session
lifetime changes are restart-required configuration changes, not settings API
fields.

### Errors

Operational errors use:

```json
{
  "error": {
    "code": "conflict",
    "message": "task changed since it was read",
    "retryable": false,
    "field_errors": []
  }
}
```

Stable codes include `invalid_request`, `unauthorized`, `forbidden`, `not_found`,
`conflict`, `unavailable`, `internal_error`, `remote_state_ambiguous`, and
`publication_ambiguous`. Login/auth endpoints use the smaller top-level form
`{"error":"unauthorized"}`, `forbidden`, `rate_limited`, or `internal`.

### SSE Semantics

`GET /api/events` is authenticated SSE. Every connection first receives:

```text
event: refetch
data: {"reason":"snapshot_required"}
```

The bounded channel has 64 entries. Active durable changes can emit
`task_updated`, `worker_updated`, or `scheduler_updated` with an `event_id` and
current representation. Worker deletion, serialization/load failure, or receiver
lag emits `refetch`. Session authentication is passively rechecked every 30
seconds without extending idle lifetime.

SSE is a bounded invalidation/refetch hint, not durable truth. Reconnect and
`refetch` mean fetch the current page, selected detail, workers, settings, and
counts as needed. History and attempts are server-paginated through HTTP. They
are never replayed through SSE. SQLite and API responses remain authoritative.

## Troubleshooting

Use public liveness to decide whether the process accepts HTTP:

```bash
curl --fail http://127.0.0.1:3001/api/health
```

Use authenticated readiness to check migrations, current password-hash access,
and retained root handles. Worker availability is intentionally not a readiness
condition, so one offline GPU host doesn't make Controller unready.

Diagnosis order:

1. Record Controller version, task ID, attempt ID, worker ID, status, failure
   stage/code/message, and timestamps. Redact Authorization, Cookie,
   `x-csrf-token`, login bodies, and hash contents.
2. Check `/api/readiness`, Workers online/enabled/compatibility state, scheduler
   pause, and task detail attempts.
3. Check that NAS roots still name the same real directories and permissions.
4. Check remote Videnoa health, `jobs.db`, matching job identity, and task
   workspace without deleting anything.
5. For publication, preserve and hash the exact final, verified temp artifact,
   and any legacy staging file named by durable evidence.

Common failures:

- `401`: missing, expired, revoked, or rotated session/Bearer credential.
- `403`: cookie mutation lacks exact same-origin proof or current CSRF header.
- `409 task changed`: refetch detail/settings/worker and review before retrying.
- Worker offline: fix network/TLS/Videnoa service health. Assignment remains
  reserved during uncertain compute; don't reassign it manually.
- Workflow incompatible: install or choose a workflow/preset on each intended
  Videnoa with exact `Path` inputs `input` and `output`.
- Input unavailable or changed: preserve the source, correct the root/path, then
  create a new task when the immutable input contract changes.
- Output exists: preserve it and create a new task with another output path.
- Cleanup failure: restore worker access and let cleanup retry. Don't rerun AI.
- Ambiguous state: follow the preservation procedures above. Never delete the
  evidence or force a retry.

## Backup and Restore

Controller SQLite runs in WAL mode with automatic SQLx migrations. For a simple,
source-accurate filesystem backup:

1. Pause scheduling through the Web UI or `/api/scheduler/pause`.
2. Let active work reach a known state, then stop Controller cleanly with
   SIGTERM, SIGINT, service stop, or console Ctrl+C. Shutdown durably pauses new
   dispatch, drains database work for up to 30 seconds, and leaves remote jobs
   running for restart reconciliation.
3. Copy the full `data_root`, not only `controller.sqlite3`. Include
   `controller.sqlite3-wal` and `controller.sqlite3-shm` if present.
4. Back up config and the protected hash file separately. Back up every worker's
   persistent Videnoa data, especially `jobs.db` and `workspace/`.
5. Record Controller and Videnoa versions with the backup, without recording
   credentials.

Linux stopped-service example:

```bash
BACKUP_ROOT="/srv/backups/videnoa-controller/$(date -u +%Y%m%dT%H%M%SZ)"
install -d -m 0700 "$BACKUP_ROOT"
cp -a /var/lib/videnoa-controller "$BACKUP_ROOT/controller-data"
cp -a /etc/videnoa-controller/controller.toml "$BACKUP_ROOT/controller.toml"
cp -a /var/lib/videnoa-controller/admin-password.phc "$BACKUP_ROOT/admin-password.phc"
```

Restore while Controller and affected workers are stopped. Restore the complete
matching Controller data snapshot, config, hash file, NAS roots, and worker data.
Keep paths and permissions consistent. Start Controller, check health/readiness,
inspect every nonterminal task and worker, then resume scheduling. Don't combine
a Controller snapshot with a different worker `jobs.db` generation for active
tasks.

## Upgrade and Rollback

Upgrade procedure:

1. Read release notes and pin the target image/version or archive.
2. Pause, drain to known states, stop, and take the complete backup above.
3. Replace only the Controller binary/image. Preserve configuration, data,
   temp, NAS roots, hash file, and all Videnoa persistent data.
4. Start the new version. Startup applies pending migrations atomically.
5. Verify `/api/health`, `/api/readiness`, Web login, workers, retained pages,
   task detail/attempt history, and nonterminal reconciliation before resume.

Rollback procedure:

1. Stop the new Controller and preserve its post-upgrade data for diagnosis.
2. Restore the full pre-upgrade Controller data snapshot and matching worker
   data. Don't reuse the migrated database with the older binary.
3. Restore the previous binary/image and previous compatible configuration.
4. Verify health, readiness, task history, workers, and active evidence before
   resuming.

There is no manual migration command and no supported migration downgrade. Don't
edit `_sqlx_migrations`, task rows, attempts, idempotency rows, or versions by
hand. A startup migration failure should leave the service stopped; restore the
snapshot or correct the storage problem before retrying the upgrade.

## Cleanup and Retention

Controller removes its temp task artifacts, then deletes the remote task
workspace. Remote DELETE `404` is already-cleaned success. Network, timeout, and
server failures retry with persisted backoff; other remote client failures can
be terminal configuration errors. A task becomes `completed` only after local
and remote cleanup converge.

Task and attempt history is retained indefinitely. Don't delete the SQLite
database as cleanup. Capacity and recovery depend on durable rows. Plan disk
retention for the database and normal service logs, and use paginated API/UI
history rather than copying or truncating tables.

## Distribution and Release

Docker images are exactly:

```text
controlnet/videnoa-controller:<version>
controlnet/videnoa-controller:latest
```

Use the workspace version tag for production. The dedicated image is Debian
bookworm slim, embeds the frontend, runs as UID/GID `10001:10001`, exposes port
3001, and uses `videnoa-controller` as entrypoint. Its default command is
`--config /etc/videnoa-controller/controller.toml --host 0.0.0.0`.

Container mounts:

| Container path | Access and purpose |
|---|---|
| `/etc/videnoa-controller` | Read-only configuration |
| `/var/lib/videnoa-controller` | Read-write SQLite data |
| `/mnt/input` | Read-only NAS input root |
| `/mnt/publication` | One read-write parent mount containing separate `temp` and `library` directories |
| `/run/secrets/admin-password.phc` | Separate read-only hash file |

A root-owned mode-0600 bind mount isn't readable by UID 10001. Use host
ownership/group/mode or a secret mechanism that permits read access while
keeping the mount read-only. Never bake the hash into an image layer.

Set the container config to `input_roots = ["/mnt/input"]`,
`output_roots = ["/mnt/publication/library"]`,
`temp_root = "/mnt/publication/temp"`,
`data_root = "/var/lib/videnoa-controller"`, and the hash file above.
Create `/srv/media/temp` and `/srv/media/library` on the same filesystem;
only `library` belongs to the Jellyfin library, never the common parent or temp.
Grant UID/GID 10001 write access to both directories.

Do not bind-mount temp and output separately: Linux can reject a rename between
different mount points with `EXDEV` even when their host directories have the
same device ID. Mount their common parent once, as below, and avoid nested
mounts between them. Cross-filesystem publication fails explicitly; Controller
never falls back to copying or creating a hidden output-side staging file.
Start the pinned image with matching mounts and no GPU flag.

```bash
docker run -d --name videnoa-controller \
  -p 127.0.0.1:3001:3001 \
  --mount type=bind,src=/etc/videnoa-controller,dst=/etc/videnoa-controller,readonly \
  --mount type=bind,src=/var/lib/videnoa-controller,dst=/var/lib/videnoa-controller \
  --mount type=bind,src=/srv/media/incoming,dst=/mnt/input,readonly \
  --mount type=bind,src=/srv/media,dst=/mnt/publication \
  --mount type=bind,src=/run/secrets/admin-password.phc,dst=/run/secrets/admin-password.phc,readonly \
  controlnet/videnoa-controller:<version>
```

Release archives are exactly:

```text
videnoa-controller-v<version>-linux-x86_64.tar.gz
videnoa-controller-v<version>-windows-x86_64.zip
```

Each archive root contains only:

```text
LICENSE
README-controller.md
controller.example.toml
videnoa-controller       # Linux
videnoa-controller.exe   # Windows
```

Frontend assets are embedded. Don't rename the executable to `videnoa` and don't
merge Controller into existing Videnoa archives or the GPU image. Linux archive
creation/verification uses `scripts/package_controller.sh`; native Windows
creation and executable proof use `scripts/package_controller.ps1` on Windows.
Real GitHub Release and Docker Hub publication are hosted workflow operations,
not local documentation smoke steps.
