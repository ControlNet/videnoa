# Controller LAN HTTP compatibility

## Contract and root cause

Plain HTTP on a trusted LAN, including IP addresses and non-localhost hostnames,
is a first-class deployment. The user explicitly requires all Controller UI
operations to work without HTTPS or public Internet exposure. Keep the default
`secure_cookie=false`. The opt-in Secure cookie setting retains its deliberate
HTTPS requirement and existing session-policy invalidation behavior.

`ManualTaskDialog` previously called `crypto.randomUUID()` before sending a task
request. That API is restricted to secure contexts; HTTP localhost tests hid the
failure on LAN HTTP. `submissionIntent.ts` now generates UUIDv4 keys from
`crypto.getRandomValues()` (available in insecure contexts), sets the version and
variant bits, and preserves the existing ambiguous-request replay behavior.
There is no Math.random fallback, dependency addition, or authentication change.

## Audit and regression coverage

- Audited production browser APIs, fetch credentials/origins, EventSource, Worker
  URL validation, listener reconnection links, backend Origin/cookie enforcement,
  and remote client schemes. No other secure-context-only production API was found.
- `playwright.http.config.ts` runs the existing browser suite on
  `http://controller-http.test:4173`, mapped to loopback by Chromium DNS rules.
  Unlike localhost, this is an insecure context. No insecure-origin security
  exemption is enabled. Vite allows that hostname only in this test config.
- The task creation regression explicitly asserts `isSecureContext=false` and
  absence of `crypto.randomUUID`, submits a UUIDv4 key, and retries a dropped
  response with identical body/key. Browser API routes there use synthetic data.
- Two existing browser-store assertions assumed Cache Storage was always exposed.
  They now accept its absence while still requiring an empty cache when available.
  Cookie fixtures use the current base URL so expiry is tested on the real origin.
- `scripts/tests/controller_http_browser_smoke.mjs` starts an isolated Docker
  container with synthetic media and an intentionally offline test worker. A real
  Chromium browser performs setup, login/logout, HttpOnly non-Secure cookie use,
  authenticated reload, CSRF rejection, live SSE connection, Settings TOML save,
  pause/resume, Worker CRUD, task create/cancel, and container restart persistence.
  It performs no GPU compute and touches no production data or running user container.
  Docker may change an ephemeral published port on restart; refresh the mapping.
- Both the HTTP suite and the real Docker/browser smoke are included in CI.

## Verification commands

```bash
cd controller-web
npm run lint
npm test
npm run test:e2e:http
npm run test:e2e
cd ..
VIDENOA_CONTROLLER_WEB_PREBUILT=1 cargo +1.83.0 test --locked -p videnoa-controller --test auth_http --test auth_bootstrap
bash scripts/tests/controller_docs_test.sh
node scripts/tests/validate_ci_release_workflows.test.mjs
docker build -f Dockerfile.controller -t videnoa-controller:dev .
bash scripts/check_controller_container.sh videnoa-controller:dev --all
node scripts/tests/controller_http_browser_smoke.mjs videnoa-controller:dev
git diff --check
```

Initial focused unit regression failed before the implementation change. Final
unit run passed 133 tests across 22 files; HTTP Chromium passed 53 tests. Backend
authentication regressions passed 14 tests. Lint, production build, documentation,
CI workflow contracts, container smoke, and real HTTP browser smoke passed.
Browser execution coverage is Chromium; no native Firefox/WebKit run is claimed.

An already-running container still uses its original image after a tag rebuild.
Recreate it with the updated image and preserve its existing bind mounts; merely
restarting the old container does not install the new frontend.

The complete localhost Chromium control suite also passed all 53 tests. Final
Docker rebuild produced image `5ed9ed21390000b0aef167db0f4124631a44a2bf4af5902b17d030e51240cc0e`,
the same image already verified by the real browser and container smoke runs.
The user's existing `videnoa-controller-test` container was left running unchanged.
