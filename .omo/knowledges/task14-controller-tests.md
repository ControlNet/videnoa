# Task 14 Controller Test Migration

- Build controller integration fixtures with `ConfigBootstrap::open`, reconcile settings into SQLite, and seed the administrator credential through `Store::insert_administrator_credential`.
- Current settings PUT requests require `version`, `server`, nested `auth`, `scheduler`, `timeouts`, and `retry`. Settings GET exposes auth values as flattened response fields and paths under `paths`.
- Assert settings projection through `data/controller.toml` as well as live scheduler state.
- To test readiness against deliberately corrupt credential material while retaining the exact invalid value, acquire one SQLite connection, enable `PRAGMA ignore_check_constraints = ON`, and execute the corrupting UPDATE on that connection.
- Keep large integration suites split by behavior and measure pure LOC; the task14 split uses support, status, auth/readiness, workers, task actions, retry support/faults, and SSE modules.
