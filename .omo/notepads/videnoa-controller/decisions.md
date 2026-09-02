# Decisions

## 2026-09-02 Task 1

- Keep Controller on `127.0.0.1:3001` by default so it does not collide with the existing Videnoa service on port 3000; expose `--host` and `--port` through the independent binary.
- Validate debug assets at startup and return typed `FrontendDirectoryMissing` or `FrontendIndexMissing` errors instead of silently starting with broken SPA routes.
- Keep Task 1 UI intentionally static and limited to product identity, boundary description, service status, and health endpoint; authenticated navigation and operations controls remain deferred to their plan tasks.
- Preserve the existing Docker image product and modify only its Cargo manifest/dummy-source cache enumeration for the new workspace member.

## 2026-09-02 Task 1 Fix Round 1

- Set `videnoa-controller` to Rust edition 2021 rather than raising the workspace MSRV.
- Return the minimal JSON body `{"error":"not_found"}` with status 404 for unknown `/api/*` paths; defer the full API error model to Task 2.
- Raise only the dark accent lightness from `0.58` to `0.61`, preserving the approved light token, layout, typography, and surface structure.

## 2026-09-02 Task 1 Fix Round 2

- Register explicit `any(api_route_not_found)` routes for `/api` and `/api/` before the wildcard API route and SPA fallback so all methods share the exact Task 1 JSON 404 contract.
- Keep the follow-up Rust-only: production behavior changes only in Controller router composition, while the other Controller Rust changes are the required rustfmt output.

## 2026-09-02 Task 1 Fix Round 3

- Decode the raw request path exactly once only for API-boundary classification; do not rewrite the URI or recursively decode percent escapes.
- Return the existing exact JSON 404 contract for encoded spellings that decode into `/api`, and return an empty 400 for malformed or invalid-UTF-8 paths.
- Apply one shared outer middleware and equivalent GET/HEAD method filters to avoid debug/release behavior drift.

## 2026-09-02 Task 1 Fix Round 4

- Store the validated one-pass decoded path as `DecodedPath(Box<str>)` in request extensions and consume it only at the release embedded-asset boundary; do not rewrite the request URI.
- Reject decoded backslashes and control characters with the existing empty non-HTML 400 response.
- Normalize release SPA fallback to exact `text/html` while leaving static MIME inference unchanged.

## 2026-09-02 Task 1 Fix Round 5

- Reject decoded segments equal to `.` or `..` and internal empty segments with the existing empty non-HTML 400 response before API classification or fallback dispatch.
- Preserve root and one ordinary trailing slash, Unicode segments, query strings, one-pass decoding, encoded hyphens, and encoded slashes that decode into a non-ambiguous path.
- Keep release lookup on the same typed `DecodedPath`; do not separately normalize either fallback.

## 2026-09-02 Task 1 Fix Round 6

- Replace debug `ServeDir` fallback with exact async disk reads driven by `DecodedPath`; do not rewrite the URI or add another decoding layer.
- Treat a non-root trailing separator as an exact-file miss and return the validated SPA index, preserving ordinary trailing-slash client routes.

## 2026-09-02 Task 1 Fix Round 7
- Build debug disk paths only by folding individually validated relative components from `ExactAssetPath`; use the same type for embedded keys.

## 2026-09-02 Task 1 Fix Round 8

- Compare numbered device suffixes as Unicode strings so superscript aliases are classified without UTF-8 byte-length assumptions.
- Keep portable request contracts active on every host and gate only mutation-sensitive special-name filesystem fixtures to Unix debug builds.
- Remove the unused `tower-http` Controller dependency and the package-wide `module_name_repetitions` allowance after strict Clippy proved neither is required.

## 2026-09-02 Task 2

- Use UUID-backed brands for task, attempt, worker, session, remote-job, submission, and SSE-event identifiers; use a distinct opaque string brand for task-ingress idempotency keys.
- Lock informational task sources to `manual` and `api`; preserve an optional exact `source_reference` string without attaching source-specific behavior.
- Keep task-list defaults at `limit=100`, `offset=0`, `sort=priority`, `direction=desc`; allow only the six planned sort fields and retain deterministic repository tie-breaking for later tasks.
- Adopt configuration defaults of health `10s`, poll `5s`, transfer `300s`, retry initial `1s`, retry maximum `60s`, and retry attempts `5`, alongside the plan-locked session and scheduler defaults.
- Permit `prefetch_per_worker=0` to disable optional staging, but reject zero compute slots and zero upload/download concurrency.
- Normalize worker base URLs to one trailing slash while preserving path content; reject non-HTTP(S), credentials, query strings, and fragments.
- Keep login input deserialize-only and redacted in `Debug`; configuration contains only the password-hash file path.

## 2026-09-02 Task 5

- Validate `Idempotency-Key` as 1 to 255 visible ASCII bytes and return stable typed errors without reflecting request data.
- Fingerprint workflow name plus params with SHA-256 over recursively canonical JSON; do not include resolved file paths or workflow document contents.
- Use `BEGIN IMMEDIATE` and commit the jobs row plus key/fingerprint before `spawn_job`, so only the database winner dispatches.
- Keep idempotency columns nullable and the unique index partial; do not fabricate mappings for legacy or unkeyed rows.
- Treat worker database loss as `remote_state_ambiguous`; persistence is an operational requirement, not a compatibility fallback.

## 2026-09-02 Task 3

- Use one Controller-owned SQLx migration and `_sqlx_migrations` as the only schema history.
- Store semantic values through domain newtypes and codecs; reject corrupt persisted enums and numeric overflows with typed errors.
- Keep list limits bounded in SQL and use stable ID tie-breakers for every supported sort.

## 2026-09-02 Task 5 Follow-up

- Classify existing keys in SQLite immediately after key/name validation and fingerprinting, before resolving the current workflow.
- Return persisted replay/conflict directly, but allow only `Missing` to continue to workflow resolution and validation.
- Keep the final transactional claim exhaustive over Created, Replayed, and Conflict; preflight never becomes the creator-election authority.

## 2026-09-02 Task 3 MSRV Scope Correction

- Keep `workspace.package.rust-version = "1.83"` as a selective default and opt in only `videnoa-controller`.
- Do not declare an MSRV for core, app, or desktop in this Task; their existing ORT/toolchain boundary is unchanged and outside Controller compatibility scope.

## 2026-09-02 Task 4

- Use independent random 256-bit session and CSRF values and persist only SHA-256 digests plus the current Argon2id hash fingerprint.
- Reload the password hash file for every login, Bearer verification, and cookie-session authentication so operator rotation takes effect without restart.
- Resolve relative configured roots once, reject symlinked root ancestors, snapshot root identity, revalidate the configured pathname before use, and perform every descendant directory open through `DirExt::open_dir_nofollow`.
- Keep path capabilities alive for the Controller runtime now; inject them into task and transfer state when those production routes are introduced by Tasks 7-13.

## 2026-09-02 Task 6

- Compile the reusable mock through one integration-test crate instead of every test that imports the older shared support module, preventing unrelated dead-code warnings under strict Clippy.
- Model transport failures below Axum routing with Hyper service errors and an erroring response stream; do not simulate network faults with HTTP status aliases.
- Redact volatile `Host` values in journals so real ephemeral listeners still produce deterministic evidence.

## 2026-09-02 Task 9 Submitting Cancellation Follow-up

- Require typed accepted/not-accepted submission reconciliation before a cancelling submitting attempt may authorize cleanup or finish.
- Keep ordinary lifecycle advancement blocked after cancellation intent; expose reconciliation as a separate service method that reuses the paired transactional CAS.
- Preserve cancellation intent across reconciliation and return only cancellation cleanup actions, never polling.

## 2026-09-02 Task 10

- Derive recovery remote paths deterministically from task ID plus immutable input/output extensions until Task 12 adds full transfer-stage evidence.
- Treat worker health failure as deferral, not task failure or reassignment, and persist bounded backoff on the worker row.
- Use a 30-second process shutdown drain bound while preserving configured health, poll, transfer, and retry values for recovery clients.

## 2026-09-02 Task 11

- Keep scheduler candidate selection read-only and route every claim through `LifecycleService::reserve`; do not add an independent assignment write path.
- Mirror the complete scheduler eligibility policy inside the atomic SQLite reservation statement so stale candidates fail as typed conflicts.
- Treat worker deletion as forbidden for both active and historical task references; disabling is the non-destructive operational alternative.
- Keep transfer permits ephemeral and settings durable: restart reloads limits and pause from SQLite but never reconstructs in-flight process-local permits.

## 2026-09-03 Task 12

- Bind upload paths and download hash/length through evidence-bearing lifecycle commands so successful stage transitions and their proof cannot commit separately.
- Keep file API targets task-owned and derived from task ID plus independent input/output extensions; persist only worker-returned workflow paths for workflow parameters.
- Use paired atomic task/attempt retry writes for transfer backoff, preserving the existing attempt and remote compute identity.
- Require a non-zero stat and exact `Content-Length`, sync the local file, and rename to the verified temp name before exposing `verifying`.
