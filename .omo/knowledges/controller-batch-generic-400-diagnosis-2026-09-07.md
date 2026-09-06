# Batch API generic 400 diagnosis

User supplied a NAS log at `2026-09-06T15:35:04.368331Z`: POST
`/api/tasks/batch`, HTTP 400, 81 ms, top-level `invalid_request`, message
`request is invalid`, and empty `field_errors`.

Inspection at `2aec45e` finds that the batch route maps every `JsonRejection` to
`TaskApiError::InvalidRequest`, discarding the extraction detail. This exact error
shape therefore points to JSON extraction before batch preview/intake, subject to
confirming the deployed NAS version. It does not identify a specific invalid field.
Possible causes include malformed JSON, missing required fields, unknown fields
(`deny_unknown_fields`), wrong field types/enum values, missing JSON content type,
or body read/size rejection. Elapsed time alone is not proof of a cause.

The route expects one `BatchPreviewRequest` object with `input_pattern`,
`output_mode` (`beside_input` or `directory`), optional `output_directory`,
`naming_mode` (`insert_extension` or `original`), `middle_extension`, `workflow`,
and integer `priority`. It does not accept a task array or an `items` wrapper.
Path/preview validation uses field errors or batch-row diagnostics instead.

Current `BatchTaskDialog` posts preview options to `/api/tasks/batch-preview`,
then creates selected tasks individually through `/api/tasks`. Do not assume a
logged `/api/tasks/batch` call came from the current bundled browser UI.

Historical `40a53dc` batch routing also collapsed JSON extraction failures to the
same generic error; unsupported Idempotency-Key then produced a distinct field
error. No NAS request body, headers, or deployed image version was available, so
the precise root cause remains unconfirmed. Next evidence: caller type, deployed
version, sanitized request JSON, and Content-Type; never collect authentication
headers/cookies. No runtime or diagnostic behavior was changed in this investigation.

## Confirmed cause and follow-up implementation

The supplied request included `source_reference`, which was not part of the batch
request schema. The unknown-field rejection explains the generic 400.

Batch preview and creation now accept optional `source_reference` and preserve it
on each generated request and persisted task. It shares single-task validation:
nonempty and at most 512 UTF-8 bytes when supplied; omitted/null means no reference.
Validation runs before preview scans or task admission. Both keyed and unkeyed
creation use the same propagation path, with no SQLite migration.

The field participates in keyed batch fingerprints when present. Serialization
omits None so historical fingerprint bytes stay unchanged; omitted and explicit
null remain equivalent. Changing a supplied reference with the same key conflicts.

Regression coverage uses synthetic media and test-only source references. Tests
cover preview, multiple unkeyed tasks, persisted task detail, keyed creation and
restart replay, changed-reference conflict, omitted/null compatibility, exact
historical fingerprint bytes, and early field-specific rejection of invalid values.

Verification commands (expect passing tests and no lint/format errors):

```sh
cargo test --locked -p videnoa-controller --test task_api
cargo test --locked -p videnoa-controller --lib
cargo fmt --all -- --check
cargo clippy --locked -p videnoa-controller --all-targets --all-features -- -D warnings
bash scripts/tests/controller_docs_test.sh
```
