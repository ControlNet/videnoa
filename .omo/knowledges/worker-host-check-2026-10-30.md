# Worker Host check against DNS rebinding (finding O, 2026-10-30)

## Problem

With no password set, `protect` passed every request and `same_origin` only
compared Origin with the request's own Host. A page on a rebinding domain
(`Host: evil.example:38123`, `Origin: http://evil.example:38123`) could save
workflows (`POST /api/workflows` returned 201), browse `/api/fs/*`, read job
WebSockets and claim the first password.

## Fix

- `auth::check_host` (`crates/core/src/server/auth/mod.rs`) is a `.layer` on
  `api_router`, after all routes, so it covers business routes, `/api/health`,
  `/api/jobs/{id}/ws`, `/api/auth/*` and the `/api/{*path}` 404. Static SPA
  assets (fallback added later in `app_router_with_static`) are not covered;
  they hold no data.
- Order: Host trusted -> pass; else auth storage unavailable -> 503; else
  password set -> pass (unchanged behaviour); else 403 with a message naming
  the host and `server.allowed_hosts`.
- Trusted Host: IP literals (v4, bracketed v6, any port), `localhost`,
  `*.localhost`, the machine's names (full hostname, first label,
  `<first label>.local`; `libc::gethostname` on unix, `COMPUTERNAME` on
  Windows, cached in a `OnceLock`), and `server.allowed_hosts` (case-insensitive,
  trailing dot ignored, port ignored). A char allowlist runs before
  `url::Url::parse`, so `evil@127.0.0.1` style values are rejected.
- A request without Host passes: browsers always send Host, and the existing
  `oneshot` tests and non-browser clients send none.
- The password state is read lock-free from `AuthService`'s `password_set`
  `AtomicBool`, not from SQLite (the DB mutex can be held during Argon2). It is
  set after the credential commit and cleared before the delete, so a stale
  read only makes the check stricter; a failed disable restores it.
- iroh: the tunnel listener uses the same `api_router`, but iroh cannot run
  without a password and `disable` stops iroh before removing the password, so
  tunnelled requests always take the "password set" branch. No
  `videnoa.internal` special case.
- Config: `ServerConfig.allowed_hosts: Vec<String>` (default empty, always
  serialized so new `config.toml` files show it), `ServerConfig::validate`
  (bare names: ASCII alphanumerics, `-`, `.`, `_`), called from
  `update_config` (400 on invalid). The web Settings page spreads the loaded
  config, so the field round-trips without UI changes. Read per request, so
  changes apply without restart.

## Tests (`crates/core/src/server/auth/tests.rs`)

- `open_access_rejects_rebound_host_on_business_and_setup_routes` (was 201)
- `open_access_rejects_rebound_websocket_upgrade` (was 404 from the handler)
- `open_access_accepts_ip_loopback_machine_and_configured_hosts`
- `host_names_reject_lookalikes_and_credentials`
- `password_protection_is_unchanged_for_any_host`
- `config::tests::server_allowed_hosts_are_optional_and_must_be_bare_names`

## Operational consequence

A passwordless Worker reached by a DNS name other than its own host name (for
example `nas.local:3000`, or any name inside Docker, where the container has a
random host name), including by a Controller, now gets 403 until the name is
added to `server.allowed_hosts` or a password is set. The Controller treats a
403 on capability requests as "worker authentication failed; check the saved
worker password" (docs/password-access.md), which is misleading for this case;
the 403 body names the real cause.
