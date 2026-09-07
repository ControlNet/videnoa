# Optional service password

Videnoa starts with open access unless a password has been configured. In WebUI
Settings, **Access protection** enables, changes, or disables the password.
Password buttons apply immediately; session configuration uses the ordinary
settings save button. Changes do not stop background processing.

The login page accepts a password without a username. Passwords must contain
1–1024 UTF-8 bytes, with no control characters. Spaces are significant. Changing
or disabling a password requires an authenticated session but does not ask for
the previous password again. Changing a password replaces the current browser
session and invalidates other sessions.

Business HTTP APIs accept either the browser session cookie or
`Authorization: Bearer <password>`. An explicitly incorrect Authorization header
does not fall back to a valid cookie. Browser mutations use same-origin and CSRF
checks. External Bearer clients do not need a browser Origin or CSRF token.
Use HTTPS when transport encryption is required.

The following remain public, including when a password is enabled:

- `GET /api/health`, preserving Docker health checks.
- `/api/jobs/{id}/ws`, including progress and textual node value previews.
- Authentication status/login and static WebUI assets.

WebSockets are intentionally unauthenticated and are not closed when the
password changes. Other business endpoints, including files and preview images,
are authenticated. The WebUI does not mount its business screens until the
access status is known.

## Storage and session settings

The selected data directory contains private `auth.sqlite3` and SQLite sidecars,
plus `auth.lock`. Only salted Argon2id password hashes and session-token digests
are stored by Videnoa. Do not publish this database or put it in version control.
Authentication storage failures fail closed. The lock prevents multiple
instances or the reset command from using the same authentication data together.

The normal `config.toml` contains public session policy, not the password:

```toml
[auth]
session_absolute_seconds = 2592000
session_idle_seconds = 604800
secure_cookie = false
```

Durations must be positive integers, with idle no greater than absolute lifetime.
There is no fixed day cap, but dates and integer values must be representable.
Existing installations obtain these defaults without enabling a password.
Sessions survive restarts. Shorter limits constrain existing sessions immediately;
log in again to obtain a longer absolute lifetime. Authenticated HTTP activity
renews idle expiry; public WebSockets do not. Changing the secure-cookie policy
invalidates previous sessions. Enable `secure_cookie` only when using HTTPS.

## Authentication endpoints

| Method and path | Input or result |
| --- | --- |
| `GET /api/auth/session` | `password_enabled`, `authenticated`; authenticated browser sessions also include `csrf_token`, `expires_at`, `idle_expires_at` |
| `POST /api/auth/login` | JSON `password`; returns session state and sets an HttpOnly cookie |
| `POST /api/auth/logout` | Revokes the current session and clears its cookie |
| `PUT /api/auth/password` | JSON `password` and `password_confirmation`; sets or changes the password |
| `DELETE /api/auth/password` | Disables protection and revokes sessions |

Expiry timestamps are Unix seconds. Auth responses are not cacheable. Cookie
mutations send the current session's `csrf_token` in `X-CSRF-Token`. Initial setup
and login require an Origin matching the request Host. Concurrent initial setup
cannot overwrite the credential created by another request. Incorrect passwords
share a per-peer failure limit: five failures in five minutes, then HTTP 429.

## Controller workers

**Add Worker** and **Edit Worker** have an optional access-password field:

- Leave an Add Worker password blank for an open worker.
- In Edit Worker, leave the field blank to keep the saved password.
- Enter a new value to replace it, or choose **Clear saved password** to remove it.

Controller persists the retrievable worker password in its private SQLite
database, without a separate encryption key. It needs the original credential to
send Bearer requests. Worker responses expose only `has_password`; passwords are
not returned by read APIs or printed in diagnostics. Protect Controller database
backups accordingly. Environment-provided secrets belong in ignored local files
such as `.env.local`, never source code or example credentials.

Worker-create JSON accepts optional `password`. Update JSON uses an omitted field
to preserve it, a nonempty string to replace it, and explicit `null` to clear it.
These changes participate in the existing worker version check. A changed
password triggers a health/capability refresh. Existing workers migrate with no
password. Authentication covers worker discovery, jobs, uploads, downloads,
cleanup, and recovery; an authentication failure never retries anonymously.

## Forgotten password

Stop the specific instance first. From its workspace, or with its exact data
path, run:

```bash
videnoa auth reset --data-dir ./data --yes
```

This restores open access and clears every session. It preserves jobs, workflows,
and configuration. The command warns about the change and requires `--yes`;
it refuses while the instance owns the authentication lock. Start the instance
again and configure a new password through Settings.

## Development isolation

The existing production service on port 3000 must not be used for development.
Do not send it test requests, attach a debugger, signal it, restart it, or modify
its data or deployment files. Use an isolated worktree, build output, and fresh
data directories. Development workers and Controller instances use loopback
ports 13000 and 13001, or independently allocated free ports; automated network
tests bind port 0. Vite proxies must point to the isolated worker. Stop only
processes created by the current test run. This feature does not deploy itself
or alter the production password.

Development proxies default to these isolated ports and can be overridden:

```bash
VIDENOA_DEV_API_URL=http://127.0.0.1:13000 npm --prefix web run dev
VIDENOA_CONTROLLER_DEV_API_URL=http://127.0.0.1:13001 npm --prefix controller-web run dev
```

## Machine authentication runtime

Controller continues sending `Authorization: Bearer <worker-password>` to protected
Worker requests, including manual processing retry, uploads, downloads, and cleanup.
Public `/api/health` receives no Authorization header. Redirects remain disabled.

The Worker keeps a successful password's SHA-256 digest only in memory under the
authentication mutex. Bearer requests compare fixed-size digests in constant time;
cache misses use the existing Argon2id verifier and failed-attempt limiter. Startup
begins with an empty cache. Successful Bearer verification populates it; committed
password creation/rotation replaces it immediately; disabling clears it. Offline
reset requires the instance lock, so an old live cache cannot survive reset. Disk
credentials remain salted Argon2id hashes. Browser login/session/CSRF behavior is
independent and unchanged.

A protected capability response of 401 or 403 marks the Worker offline and pauses
periodic probes for that registration version with the error "worker authentication
failed; check the saved worker password". Editing the registration unblocks a fresh
probe immediately. Controller restart clears these in-memory blocks. Network,
429, and other capability errors retain ordinary retry/backoff behavior.
