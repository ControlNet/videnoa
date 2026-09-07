# Videnoa Controller Task 18

## Frontend Contracts

- Workers use strict Zod boundaries for `GET/POST /api/workers`, `PUT/DELETE /api/workers/{id}`, and enable/disable actions. Every mutation sends the current optimistic `version`; conflicts trigger one authoritative list refetch, and an open edit dialog consumes the refreshed worker version without discarding field edits.
- Worker API URLs accept only credential-free HTTP(S) URLs without query strings or fragments. Compute slots are limited to `1..65535`.
- Settings load `GET /api/settings` and public `GET /api/readiness` together. Mutable scheduler, timeout, and retry values remain separate from restart-required paths, session policy, and password-hash file location.
- Scheduler pause stops new reservations and uploads. Processing, polling, downloads, verification, publication, cleanup, and cancellation continue.

## UI And Evidence

- Workers use a dense semantic table whose frame owns horizontal overflow. Task 18 evidence includes left, middle, and action-column captures at narrow width so hidden columns are proven reachable.
- Settings use ruled sections with adjacent bounded numeric validation. Responsive evidence captures Scheduler capacity, Timeouts, Retry policy, save/version controls, and the complete read-only/readiness section at 1280, 768, and 375 CSS-pixel widths.
- Deterministic Playwright API fixtures are test-only. The mutation journal proves worker create/update/disable/delete versions, stale worker/settings refetches, client-side URL/numeric rejection, and settings save/pause/resume request paths.
- Safe operator guidance maps durable-reference deletion conflicts to disabling the worker instead of exposing a raw backend message.

## Verification

- `npm run test`: 94 tests passed.
- `npm run test:e2e`: 31 Chromium scenarios passed.
- `npm run typecheck`, `npm run lint`, and `npm run build` passed.
- Fresh evidence is stored under `.omo/evidence/videnoa-controller/task-18/`.
