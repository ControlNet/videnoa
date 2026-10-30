# Web editor / worker API review fixes (2026-10-30)

Fixes for findings D, T, the worker side of C, the UI side of G and the
Low items (settings whole-document save, Print replay, i18n, silent
failures) from `design-review-2026-10-30.md`.

## Worker (`crates/core/src/server/mod.rs`)

- `GET /api/presets` reads `load_builtin_presets(config.paths.presets_dir)`
  on every call (sorted by id), so it follows the configured directory.
  `POST /api/presets` had no caller (web, controller, scripts, docs) and
  only added to an in-memory list that `/api/run` never read; it is
  removed (now 405). `AppStateInner::presets` stays (dead, allowed) so
  `AppState::new` keeps its signature.
- `POST /api/workflows` accepts optional `filename` (plain `*.json`) and
  `overwrite` (default false). Without `overwrite` the file is created
  with `create_new`; an existing file is `409 workflow_exists`. A
  filename that is also a preset run name (`presets_dir/<filename>`
  exists) is `409 workflow_name_is_preset`, since `/api/run` resolves
  presets first and the saved workflow would never run.
- New `AppError::Conflict { code, message }` (409) and
  `AppError::PreconditionFailed { code, message }` (412).
- `GET /api/config` returns `ETag` (quoted sha256 of the JSON body).
  `PUT /api/config` with `If-Match` compares under `settings_lock`;
  a mismatch is `412 config_changed`. No `If-Match` = unconditional
  (older clients keep working). The PUT response carries the new ETag.
- Print replay: `AppStateInner::debug_values` keeps the latest debug
  event per node per job; the job WebSocket sends them right after
  subscribing, so a subscriber that joins after a Print fired still sees
  it. Entries for jobs with no live progress sender are pruned when a
  new job records its first value. The record call sits in `run_job`'s
  node-debug callback (before the throttle).

Tests: `test_list_presets_follows_the_configured_presets_dir`,
`test_presets_cannot_be_created_through_the_api`,
`test_save_workflow_is_create_only_unless_overwrite_is_set`,
`test_save_workflow_writes_the_given_filename`,
`test_save_workflow_rejects_a_preset_run_name`,
`test_update_config_rejects_a_stale_if_match`,
`test_job_ws_replays_print_values_to_late_subscribers`.

## Web

- Undo history (`past`/`future`) is reset by `loadWorkflow` and
  `clearWorkflow`, so undo can no longer put another file's canvas under
  the loaded `currentFile`.
- Save sends `filename` of the loaded file with `overwrite: true`; "save
  as new" is create-only and a 409 `workflow_exists` opens a replace
  confirmation; 409 `workflow_name_is_preset` shows a toast.
- Dirty state is a fingerprint of nodes/edges/params (derived Workflow
  interface params excluded) compared with the last saved/loaded one
  (`selectIsDirty`, `markSaved`). Loading a preset/workflow and Clear ask
  through `DiscardChangesDialog`; `beforeunload` is armed while dirty.
- `authenticatedFetch`: a 403 on a non-safe, non-`/api/auth/*` request
  that carried a CSRF token re-reads `/api/auth/session` once (shared
  in-flight promise) and retries once. Password changes are announced on
  `BroadcastChannel('videnoa-auth')`; other tabs refresh their session.
- Settings page and the desktop locale write send `If-Match` with the
  ETag from `getConfig()`; on 412 the page reloads the config and says
  so, the locale write re-reads and retries once.
- Nodes whose type `/api/nodes` does not know render as a deletable
  placeholder; params a node does not declare are listed as unsupported
  with a remove button. Params keep JSON values (`NodeParamValue`)
  instead of being stringified. `/api/nodes` load errors show a banner
  with Retry; definitions are refetched on window focus.
- Silent failures now surface: `browseDirectory`/`listDirectory` throw
  `ApiError` on non-2xx (PathAutocomplete shows "Could not list this
  folder"); the preset gallery loads both lists with `allSettled` and
  shows a per-list error with Retry; a Workflow node whose interface
  fetch fails keeps its stored ports and shows an error marker.
- i18n: remaining literals translated; auth errors go through
  `authErrorKey` (unknown -> `requestFailed`); `zhCNResources` is typed
  from `enResources` keys (missing/extra key = tsc error) and
  `locale-parity.test.ts` also checks interpolation placeholders.

Test files: `stores/__tests__/workflow-store.test.ts`,
`pages/editor/__tests__/{EditorToolbar.save,editor-unsaved-changes,EditorPage,CustomNode,PresetGallery}.test.tsx`,
`auth/AuthGate.test.tsx`, `pages/settings/__tests__/SettingsPage.test.tsx`,
`i18n/__tests__/{desktop-locale,locale-parity}.test.ts`,
`stores/__tests__/node-definitions-store.test.ts`,
`components/shared/__tests__/PathAutocomplete.test.tsx`,
`api/__tests__/client.test.ts`, `pages/jobs/__tests__/JobsPage.test.tsx`.

## Not changed

- `WorkflowPathPicker` still turns a failed preset/workflow list into an
  empty select (same pattern, not in the reviewed list).
- `compile.rs` and `config.rs` untouched (other owners).
