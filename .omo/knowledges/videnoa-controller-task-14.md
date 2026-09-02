# Videnoa Controller Task 14

## Operational API composition

- The complete Controller router composes authentication, task intake/history, and operations routes over one `AuthService`; the legacy authenticated-only router keeps its narrower readiness response for its existing tests.
- Operational reads require existing bearer or session authentication. Mutations use the existing authorization boundary, so cookie sessions additionally require same-origin and CSRF proof while bearer requests remain CSRF-exempt.
- Worker CRUD, enable/disable, settings update, scheduler pause/resume, task cancel/retry, grouped status counts, composed readiness, and SSE are all same-origin Controller endpoints. No route calls Videnoa directly.

## Durable mutation contracts

- Worker and settings mutations preserve the existing zero-based optimistic versions. Stale worker updates, enable/disable requests, deletes, settings updates, and pause/resume requests return typed HTTP `409` responses.
- Worker deletion delegates to the registry's durable reference check, so active or historical task references prevent deletion.
- Task cancellation and downstream retry delegate to `LifecycleService`; illegal, completed, cancelled, ambiguous, and stale actions retain lifecycle policy instead of adding route-local transitions.
- Processing retry queries the assigned Videnoa job, requires a terminal remote status, converges remote workspace deletion, then supplies typed terminal and cleanup evidence to `LifecycleService::retry_processing`. Nonterminal, unavailable, or ambiguous remote state returns a typed error without creating a new attempt.

## Readiness and live updates

- Operational readiness authenticates first, then checks the migrated settings row, current password-hash file, and retained input/output root identities. Worker online state is intentionally excluded.
- Status counts are grouped by durable task status in SQLite and summed with checked arithmetic.
- `EventHub` uses a bounded 64-entry Tokio broadcast channel. Each new connection receives `refetch`; lagged receivers also receive `refetch` without replaying history.
- A shared durable-change observer connects every `Store` clone to the operations hub, so scheduler settings, lifecycle, recovery, transfer, and worker-health service mutations emit post-commit active-state deltas. Worker deletion emits `refetch` because the SSE contract has no deletion delta variant.
- SSE revalidates on a persistent 30-second interval even under continuous traffic. Cookie sessions are checked passively so an open event stream cannot extend idle expiry.

## Verification

- Thirteen Task 14 integration tests cover authentication, cookie CSRF, worker optimistic concurrency, scheduler conflicts, cancellation/retry policy, processing retry cleanup and fresh identity, readiness failure, HTTP and background SSE deltas, deterministic lag handling, and bounded settings.
- Formatting, strict Clippy, Controller build, the full Controller suite, changed-file diagnostics, and live authenticated HTTP/SSE probes pass.
- Every touched production Rust module remains below 250 nonblank, non-comment lines.
