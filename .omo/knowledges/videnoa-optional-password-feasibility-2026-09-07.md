# Optional Password for the Videnoa Service

## Scope and findings

- Assessment only; no authentication feature has been implemented.
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
- Only authentication bootstrap/login status and required static assets are public by default; any public health exemption must be explicit and minimal.
- Password changes/removal require authenticated authorization, with current-password confirmation recommended. Rotation revokes old sessions and existing authenticated WebSockets; disabling returns the service to open access.
- Persist only a salted password hash with atomic updates. Corrupt/unreadable credential storage must fail closed rather than silently restore open access.
- Cover CSRF and WebSocket Origin checks, failed-attempt limits, bounded password verification, concurrent initial setup, and transport security. The service currently defaults to listening on all interfaces, so initial open access permits reachable clients to claim password setup.

## Estimate and verification scope

- Engineering estimate, not a measured delivery guarantee: roughly 2–4 focused days for service authentication, WebUI integration, and regression tests; about 1–2 additional days for Controller protected-worker support.
- Verify open/protected/disabled states, restart persistence, bad and missing Bearer credentials, cookie expiry and revocation, config secrecy, files/previews/WebSockets, concurrent setup and rotation, and desktop behavior.
- Future implementation gates: `cargo test --locked -p videnoa-core`, `cargo fmt --all -- --check`, `cargo clippy --locked -p videnoa-core -p videnoa-app --all-targets -- -D warnings`, `npm --prefix web test`, `npm --prefix web run lint`, and `npm --prefix web run build`, using the environment from AGENTS.md.
- This assessment used source inspection only; no runtime tests were run.
