# Controller Tasks view state

- The Tasks page derives filters, sorting, pagination, and optional columns from URL query parameters in `controller-web/src/tasks/query.ts`.
- `TasksPage` updates those parameters with React Router `setSearchParams(..., { replace: true })`; it does not use browser storage or a Controller persistence API.
- Non-default selections survive refreshes and Controller container restarts as long as the browser retains the parameterized URL. Opening bare `/tasks` restores defaults.
- URL state remains useful for bookmarks and shared diagnostic views. If durable per-browser preferences are added, use URL values first and local storage only as a fallback for absent parameters.
- Good preference candidates are optional columns, status, sort, order, and row count. Avoid persisting transient search text and pagination offset.
