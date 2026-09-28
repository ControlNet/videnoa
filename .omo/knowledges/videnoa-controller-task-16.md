# Videnoa Controller Task 16

## Surface

- The production `/tasks` route renders a bounded server-paginated history table with status, workflow, worker, search, sort, order, row-limit, paging, and optional-column state encoded in the URL.
- The browser accepts only limits `25`, `50`, or `100`. It fetches one
  `/api/tasks` page for each load trigger and never materializes full task
  history.
- `/api/status-counts` is global, so it is read once per mounted view, not per
  filter, sort or page trigger.
  - It is re-read on invalidation, on retry, and on any `task_updated` event,
    including events outside the current filter.
  - Events are coalesced into a trailing read one second later, with one read
    in flight at a time.
  - This was changed in `48a6335` (second review, finding 6), and the E2E
    expectations were updated on 2026-09-28.
- The table owns horizontal overflow. Filters wrap on desktop and reflow into a two-column grid on narrow screens so the document and toolbar remain horizontally bounded.

## Live Updates

- `task_updated` SSE payloads use the same strict task schema as list and detail responses.
- A row is replaced only when the incoming version is newer and its filter membership and ordering keys are stable.
- Matching updates outside the current page, or updates that change
  membership or order, invalidate the bounded page.
- Unrelated updates outside a filtered page leave the page alone but still
  refresh the global counts.

## Backend Contract

- Task summaries include `version`, `input_size`, and latest-attempt `remote_job_id`.
- Persistence projects the latest remote job with the highest `attempt_no`, preserving one summary shape across list, detail, and SSE.

## Verification

- Unit tests cover URL defaults/bounds, all-status counter aggregation, and monotonic merge rules.
- Playwright covers a simulated 20,000-task history, one bounded page request per search/sort/page trigger with a single counts read, URL persistence, desktop and narrow overflow ownership, recoverable load failure, and deterministic evidence capture.
- Evidence lives under `.omo/evidence/videnoa-controller/task-16/`.
