# Controller Tasks view state

- The Tasks page keeps its browser URL at `/tasks`; changing filters, sorting, pagination, and optional columns does not add query parameters.
- Durable view controls are stored under the versioned browser-local key `videnoa.tasks.view.v1`: optional columns, status, source, failure stage, workflow, worker, sort, order, and row count.
- Search text and pagination offset remain session-only state and reset on a page reload.
- An old parameterized Tasks link is read once for compatibility, saved as browser-local preferences, and immediately replaced with the clean route.
- This state does not require a Controller persistence API or database migration. It survives Controller container restarts because it belongs to the browser profile.
