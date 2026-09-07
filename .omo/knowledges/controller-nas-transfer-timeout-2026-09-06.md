# NAS transfer failure and timeout correction

## Report and observed evidence

- User reported upload-stage `transfer_failed`, retry count 5, with no remote job/input/output persisted for the failed attempt. Controller is on the home NAS; Worker runs on this machine.
- The local Worker was listening on port 3000. Its `/api/health` returned 200 and a nonexistent `/api/files/.../stat` returned the expected 404. Workspace disk had approximately 258 GB available.
- A connection from the NAS and a recent partial workspace upload were observed. The file size changed, shrank, and later disappeared, consistent with partial-upload retries/cleanup. This file could not be definitively correlated with the supplied attempt because the NAS database/configuration was not available; the attempt was absent from the local Controller database.
- No live task was created, no live file was overwritten/deleted by the agent, and neither deployment was restarted. Credential-related log lines were excluded from diagnostic output.

## Root cause found in source

- `scheduler/runtime_settings.rs` maps `poll_seconds` to `RemoteTimeouts.request` and `transfer_seconds` to `.stall`.
- Both upload and download incorrectly used `.timeout(self.timeouts.request)`, applying the default 5-second control-request timeout to the entire transfer, including body streaming.
- Default 300-second transfer timeout only guarded response chunks and therefore did not protect uploads from the 5-second cutoff. Retry reconciliation deletes incomplete uploads and restarts from zero.
- Red-first real TCP regression set a 75 ms request timeout and 2 s transfer timeout, held upload acceptance for 200 ms, and failed with `Timeout` on old code.

## Fix

- Upload requests now use the transfer timeout for the entire request.
- Download no longer carries the short overall request deadline. Response-header wait and each body read are independently bounded by the transfer timeout, allowing active downloads to last longer.
- Control requests retain the poll timeout. No schema/config migration or Worker change is needed.
- An additional test confirms upload still times out at its transfer deadline. Existing stalled/truncated download tests retain distinct errors.
- On an old NAS build, temporarily increasing Poll timeout seconds beyond the expected transfer duration is a workaround. After updating Controller, use Transfer timeout seconds above the expected complete upload duration; its default remains 300 seconds. Retry the existing failed task after correction.
- Fix is a source change for Controller, not a deployed NAS update or newly published Docker image.

## Verification

- `cargo +1.83.0 test --locked -p videnoa-controller --test videnoa_client`: 19 passed.
- `cargo +1.83.0 test --locked -p videnoa-controller --test task12 --test task13`: 24 + 42 passed (85 total across these targets).
- `cargo +1.83.0 clippy --locked -p videnoa-controller --all-targets --all-features -- -D warnings`: passed.
- Workspace fmt, direct rustfmt on transfer/config modules, `bash scripts/tests/controller_docs_test.sh`, and `git diff --check`: passed.
- Tests use existing mock TCP workers and explicitly synthetic bytes; no real media processing was performed.

## Superseding correction (2026-09-07)

See [Controller input scans and transfer inactivity correction](controller-input-scans-transfer-inactivity-2026-09-07.md).
The historical behavior above is retained as a record. Duplicate intake/upload
hashes are removed, and upload uses an inactivity watchdog. Any advice above to
set transfer timeout beyond the complete upload duration is superseded; the
900-second default now bounds inactivity, not total transfer duration.
