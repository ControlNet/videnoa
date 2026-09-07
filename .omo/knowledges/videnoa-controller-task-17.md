# Videnoa Controller Task 17

## Manual Intake

- `POST /api/tasks` submits exact `input_path`, `output_path`, `workflow`, integer `priority`, `source: "manual"`, and `source_reference: null`.
- Creation intent exists only in React memory. `crypto.randomUUID()` creates one idempotency key for one canonical body; only an ambiguous network response permits unchanged replay with that key.
- Editing any field after ambiguity clears the replay intent immediately. The next submit receives a new key, and a server key/body conflict is shown as a safe new-intent recovery path.
- Server field errors use the exact Rust enum: `required`, `invalid_value`, `unknown_value`, `out_of_range`, and `conflict`. Root and output-collision guidance is derived from structured field/code/message evidence while adjacent server messages remain attached to their controls.

## Authoritative Detail

- Selecting a task fetches `GET /api/tasks/{id}` and renders general data, progress, persisted attempts, retry metadata, remote evidence, timestamps, and failure guidance in the bottom inspector.
- A selected-task SSE update invalidates detail and triggers another authoritative fetch. List payloads are not projected into attempt history or action versions.
- Cancel and retry send the version displayed by authoritative detail. HTTP 409 performs one selected-detail refetch and one bounded page/count refresh before another action.

## Lifecycle Actions

- Cancellation is available from queued through verifying only while `cancel_requested_at` is null. Confirmation starts on `Keep Task`, traps focus between its actions, and consumes Escape without closing detail.
- Retry appears only when the task is failed, persisted failure evidence is explicitly retryable, and its pair is one of: `processing_failed/processing`, `transfer_failed/upload`, `transfer_failed/download`, `verification_failed/verification`, `publication_failed/publication`, `cleanup_failed/local_cleanup`, or `cleanup_failed/remote_cleanup`.
- `publication_ambiguous` and `remote_state_ambiguous` always block automatic retry and provide manual verification guidance.
- Attempt `submission_key` values parse as UUIDs; remote input and output paths remain nullable opaque strings.

## Verification

- Vitest covers request headers, strict create/detail/attempt schemas, canonical submission intent, lifecycle policy, and component rendering.
- Playwright covers lost-response replay, changed-intent key rotation, structured root/no-clobber errors, authoritative attempts, late/repeated cancellation blocking, confirmation focus containment and nested Escape, exact bounded 409 refresh, safe/blocked retry, selected-task SSE invalidation, focus restoration, and narrow containment.
- Evidence lives under `.omo/evidence/videnoa-controller/task-17/`.
- The visual gate requires exactly eight fresh PNGs with unique SHA-256 hashes; a separate bottom-scroll capture was removed because it duplicated the attempts/error-logs frame byte-for-byte.
- Replay success evidence must keep the submitted request and authoritative response fixture identical across input path, output path, workflow, priority, and source; the focused scenario asserts each rendered field and one unchanged idempotency key/body pair.
