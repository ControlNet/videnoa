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
