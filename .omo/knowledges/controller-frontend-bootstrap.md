# Controller Frontend Bootstrap And Settings

## Authentication Bootstrap

- The browser must call `GET /api/auth/setup` before `GET /api/auth/session`. An uninitialized Controller renders first-admin setup without making a session request.
- Setup uses `{ password, password_confirmation }` and validates 12 through 1024 UTF-8 bytes with `TextEncoder`, not JavaScript character count.
- HTTP 409 from `POST /api/auth/setup` is a bootstrap race, not a terminal setup-form error. Recheck setup status and session immediately.
- If the race recheck is unauthenticated, transition to sign-in with the notice `Controller setup was completed elsewhere. Sign in with the administrator password.` and return a failed setup result so the unmounted setup form does not report success.
- Setup and sign-in password inputs receive initial focus. Validation and recovery errors move focus to the first actionable invalid field or alert.

## Settings Contract

- `GET /api/settings` exposes editable `server`, authentication policy, scheduler, timeout, and retry values plus read-only `paths.workspace`, `paths.data_root`, and `paths.config_file`.
- `PUT /api/settings` nests cookie and session lifetime fields under `auth`; fixture responses must project those fields back to the flat GET response shape.
- Successful saves replace the local form from the authoritative response, display the returned settings version, identify the written configuration file, and show a reconnect link only when the listener address changed.
- A retryable `503 unavailable` Settings response means the update committed and applied while configuration projection repair is pending. Keep the degradation alert, clear the success receipt, and refetch so the form and version reflect the authoritative committed state. Disable save and pause/resume while that refetch is pending so no stale settings version can be submitted. If that exact save changed the listener bind, keep its manual reconnect link in the alert without reusing it for later mutation errors.
- Paths are operational context only. The UI must not expose legacy root or credential-path controls.

## Browser Fixtures And Verification

- Every authenticated Playwright fixture must install both `GET /api/auth/setup -> { initialized: true }` and a valid session response. Centralize this with `installAuthenticatedSession(page)` where task fixtures share the flow.
- Settings fixture PUT handlers should consume the exact nested request shape and return the incremented authoritative GET shape; this catches frontend/backend contract drift.
- Validate setup races at both desktop and narrow widths because the notice length changes the authentication panel height and wrapping.
- Expected synthetic 409 and 401 responses can appear as Chromium resource errors during race QA. A clean authenticated Settings run should have no console errors.
- Current focused verification commands are:

```bash
cd controller-web
npm test
npm run typecheck
npm run lint
npm run build
npx playwright test --project=chromium
```

- Expected signals: 132 unit tests pass, TypeScript and ESLint exit zero, the production build succeeds, and all 52 Chromium E2E tests pass. The build may emit the known non-fatal Zod annotation-position warnings.
