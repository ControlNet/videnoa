# Optional Password for the Videnoa Service

## Scope and findings

- Historical assessment; implementation now exists on `feat/optional-password`. See the implementation knowledge note for verified behavior.
- The ordinary service uses `crates/core/src/server/mod.rs` and `web/`, distinct from the already authenticated Controller.
- `app_router_with_static` centralizes HTTP endpoints, including job WebSockets, workspace file transfers, filesystem browsing, and preview images. It currently uses permissive CORS without authentication middleware.
- `AppConfig` has no authentication state. The config GET/PUT endpoints serialize the entire public configuration, so credential material should have separate private persistence and mutation endpoints.
- `web/src/App.tsx` mounts the application immediately. `web/src/api/client.ts` contains both a common request helper and direct fetch calls, plus native browser WebSockets. Preview components use image elements.
- Controller authentication provides patterns for Argon2id, bounded blocking password work, session cookies, CSRF/origin validation, credential rotation, and failed-attempt limiting. It depends on Controller-specific storage and mandatory initialization, so it cannot simply be enabled for the ordinary service.
- Desktop uses the same core router and requires regression coverage.
- `crates/controller/src/remote/client.rs` constructs worker clients without a credential argument or default Authorization header. Supporting password-protected workers also requires Controller credential configuration, private storage, and outbound request integration.

## Proposed behavior

- Missing credentials mean open access; explicitly enabling a password protects business APIs and the WebUI immediately and across restart.
- Browser login should issue a random HttpOnly session cookie, supporting same-origin HTTP requests, image loads, and WebSocket handshakes without storing the raw password in browser storage or URLs.
- External HTTP clients use `Authorization: Bearer <password>`; the notation denotes user-supplied input, not an actual credential.
- Authentication status/login, required static assets, `/api/health`, and `/api/jobs/{id}/ws` are explicitly public.
- Password changes/removal require authenticated authorization without current-password confirmation. Rotation revokes old browser sessions. WebSockets stay anonymous and connected; disabling returns the service to open access.
- Persist only a salted password hash with atomic updates. Corrupt/unreadable credential storage must fail closed rather than silently restore open access.
- Cover CSRF and WebSocket Origin checks, failed-attempt limits, bounded password verification, concurrent initial setup, and transport security. The service currently defaults to listening on all interfaces, so initial open access permits reachable clients to claim password setup.

## Estimate and verification scope

- Engineering estimate, not a measured delivery guarantee: roughly 2–4 focused days for service authentication, WebUI integration, and regression tests; about 1–2 additional days for Controller protected-worker support.
- Verify open/protected/disabled states, restart persistence, bad and missing Bearer credentials, cookie expiry and revocation, config secrecy, files/previews/WebSockets, concurrent setup and rotation, and desktop behavior.
- Future implementation gates: `cargo test --locked -p videnoa-core`, `cargo fmt --all -- --check`, `cargo clippy --locked -p videnoa-core -p videnoa-app --all-targets -- -D warnings`, `npm --prefix web test`, `npm --prefix web run lint`, and `npm --prefix web run build`, using the environment from AGENTS.md.
- This assessment used source inspection only; no runtime tests were run.

## Confirmed scope and WebSocket coverage

- The user confirmed that Controller Add Worker and Edit Worker must expose a password input and persist the worker credential for outbound authentication. Keep the design close to existing Controller patterns.
- Distinguish server-side password verification from outbound credentials: the service can store a one-way hash, but Controller needs a retrievable worker credential to send the required Bearer password. Its own administrator password hash storage cannot directly satisfy that requirement.
- The ordinary service has one WebSocket route: `/api/jobs/{id}/ws`. It pushes `progress` (current/total frames, FPS, ETA) and `node_debug_value` (node identity, textual value preview, truncation metadata).
- The browser job store subscribes to one active job at a time, updates progress and node runtime previews, and closes the old subscription when switching jobs. The API client retries disconnected sockets up to three times with a two-second delay.
- The server currently checks job/channel existence before upgrade, with no authentication or Origin check. Incoming application messages are ignored; this socket does not submit or cancel jobs, transfer files, or carry preview image bytes.
- Controller queries worker progress through HTTP `GET /api/jobs/{id}`. Its own WebUI uses SSE via `/api/events`, not the worker WebSocket.
- The user explicitly exempted WebSockets from authentication. Preserve anonymous handshakes and existing connections across password changes. Background inference continues when a viewer disconnects.
