# Deep Review Fixes (2026-11-23)

Follow-up formatting and deep-review fixes on `dev` (after `1a0d4ea`).

> Note: `Dockerfile` (`ort-download` stage fetching `microsoft/onnxruntime` v1.23.2) was intentionally left unchanged while waiting for upstream `github.com/microsoft/onnxruntime` to return from temporary private/404 status.

## Summary of Fixes

| # | Area | Severity | Root cause | Fix & regression tests |
|---|---|---|---|---|
| 1 | Controller `include!()` formatting | P0 (CI) | `cargo fmt --all` does not follow `include!()` in `crates/controller/src/module_topology.rs`, leaving 5 files touched in `c6b7b63` failing CI's `git ls-files -z '*.rs' \| xargs -0 -n1 rustfmt --edition 2021 --check` step. | Formatted `task_query.rs`, `download.rs`, `publication.rs`, `recovery_dispatch.rs`, and `transfer_executor.rs` with `rustfmt --edition 2021`. Verified against the exact CI check command. |
| 3 | Worker idempotency key lookup race | P1 | `GET /api/jobs/by-idempotency-key/{key}` did not acquire `state.lock_idempotency_key(key)` while `POST /api/run` was between `claim_idempotent_job` (`status = 'admitting'`) and `commit_admitted_jobs`. A concurrent lookup returned premature `404 JobNotFound`, which Controller `cancel_submission` treats as definitive `NotAccepted` (deleting the worker workspace right before the worker spawns the GPU job). | `get_job_by_idempotency_key` now awaits `state.lock_idempotency_key(key.as_str())` before reading `jobs_persistence`, waiting for any in-flight claim on that key to either commit (`200 OK`) or roll back (`404 JobNotFound`). Tests in `crates/core/src/server/tests/idempotency/lookup.rs` and `crates/core/src/server/mod.rs`. |
| 4 | Worker preset path resolution drift | P1 | `GET /api/presets` (`list_presets`) and `POST /api/workflows` (`save_workflow`) read raw `config.paths.presets_dir` instead of `config.paths.resolved_presets_dir()`. Launching a release bundle or desktop binary outside its install directory yielded an empty preset list and disabled preset-name shadowing protection even though `/api/run` resolved presets via `asset_base_dir`. | Switched both handlers to `resolved_presets_dir()`. Test: `test_list_presets_and_save_workflow_resolve_relative_presets_dir_via_asset_base` in `crates/core/src/server/mod.rs`. |
| 5 | Controller Web manual task double-submit | P2 | `ManualTaskDialog.submit()` lacked a synchronous ref guard, so rapid double-Enter before React rendered `disabled={submitting}` invoked `beginSubmission(null, body)` twice, generating two distinct `Idempotency-Key` UUIDs and creating duplicate tasks. | Added `submittingRef` guard in `controller-web/src/tasks/ManualTaskDialog.tsx` (matching `BatchTaskDialog`). Test in `controller-web/src/tasks/ManualTaskDialog.test.tsx`. |
| 6 | Controller Web `useWorkersData` starvation & stale overwrite | P2 | Every `taskUpdate` SSE event bumped `retryGeneration` and aborted any in-flight `GET /api/workers`, risking starvation under multi-worker task progress. Meanwhile `setLoading(true)` ran in `queueMicrotask` and `GET` responses overwrote state unconditionally without replaying in-flight worker deltas. | Added `inFlightRequest` + `pendingTaskRefresh` coalescing (at most one GET in flight + one trailing refresh) and `requestStartGeneration` delta replay (`reconcileWorkerDeltas`) in `controller-web/src/workers/useWorkersData.ts`. Tests in `controller-web/src/workers/useWorkersData.test.tsx`. |
| 7 | Worker `DownloaderNode` credential leak & cancellation | P2 | `redacted_url_for_display` only redacted query strings, leaving `user:pass@` URL userinfo intact in `Job.error` persisted in `jobs.db` and served via `/api/jobs`. Downloads and retry sleeps also ignored `ctx.check_cancelled()`. | Passed formatted display URLs through `crate::logging::redact_sensitive_text`, added `ctx.check_cancelled()` before each attempt, during polled retry backoff (`sleep_unless_cancelled`), across 64 KiB body reads, and before atomic rename (with `.part` cleanup on drop). Tests in `crates/core/src/nodes/downloader.rs`. |
| 8 | Worker iroh loopback HTTP header timeout | P2 | Internal iroh loopback API listener in `crates/core/src/server/iroh.rs` still used raw `axum::serve`, bypassing `HttpServer`'s `HEADER_READ_TIMEOUT` (30s) protection used on the main TCP listener. | Switched `iroh.rs` to `super::http_server::HttpServer::new(listener, router).serve_with_graceful_shutdown(...)`. Verified with existing iroh lifecycle/relay tests. |
| 9 | Worker Web `EditorPage` polling rejections & `Ctrl+Z` scope | P3 | `EditorPage` polled `void fetchJobs()` every 2s without `.catch(() => undefined)`, producing unhandled Promise rejections when the Worker was unreachable. Global `keydown` also intercepted `Ctrl/Cmd+Z` inside editable form inputs and required uppercase `"Z"` for Shift+Ctrl+Z redo. | Added `.catch(() => undefined)` on background `fetchJobs()` calls, ignored undo/redo inside `<input>`/`<textarea>`/`<select>`/`[contenteditable]`, and matched `e.key.toLowerCase() === "z"`. Tests in `web/src/pages/editor/__tests__/EditorPage.test.tsx`. |
| 10 | Worker upload staging `parent` + `sync_all()` & sidecar mode | P3 | `upload_file` in `crates/core/src/server/files.rs` passed the pre-`create_dir_all` `parent` to `tempfile_in` and persisted without `sync_all()`. `write_pending_terminal_sidecar` in `crates/core/src/server/persistence.rs` widened internal sidecars to `0o666`. | Re-derived `parent` from the post-`create_dir_all` `resolve_workspace_path` result, added `sync_all().await` before `persist`, and kept default private `0o600` mode on pending terminal sidecars (asserted on Unix in `crates/core/src/server/persistence/tests.rs`). |
| 11 | Controller Web `useTaskDetail` `loadMore` aborted on 1 Hz SSE | P3 | Same-task SSE progress updates invoked `reload()` -> `cancelHistoryRequest()`, aborting in-flight `loadMore` attempt-history requests. | Split `refreshSameTask` from `reload`, preserved in-flight `loadMore` across same-task SSE refreshes when the head attempt ID and offset remain unchanged, aborted/cleared it when superseded, and retained newly loaded attempts against live `detailRef.current`. Tests in `controller-web/src/tasks/useTaskDetail.test.tsx`. |
| 12 | `RUSTSEC-2026-0592` (`url` 2.5.8) | P0 (CI / Security) | `cargo deny check advisories licenses` flagged `url 2.5.8` for stack overflow in `Url::origin()` on nested `blob:` URLs (`RUSTSEC-2026-0592`). | Updated `url` to `2.5.9` in `Cargo.lock` (`cargo update -p url`); `cargo deny --locked check advisories licenses` now passes (`advisories ok, licenses ok`). |

## Verification

```bash
export ORT_DYLIB_PATH=$PWD/lib/libonnxruntime.so
export TRT_LIBS=~/miniconda3/envs/anime/lib/python3.13/site-packages/tensorrt_libs
export LD_LIBRARY_PATH=$TRT_LIBS:$PWD/lib:~/miniconda3/envs/anime/lib:${LD_LIBRARY_PATH:-}
export PKG_CONFIG_PATH=~/miniconda3/envs/anime/lib/pkgconfig:${PKG_CONFIG_PATH:-}
export PATH=$PWD/bin:$PATH
ulimit -n 65536

(cd controller-web && npm run lint && npm test -- --run && npm run build)
(cd web && npm run lint && npm test && npm run build)
cargo fmt --all -- --check
git ls-files -z '*.rs' | xargs -0 -n1 rustfmt --edition 2021 --check
VIDENOA_WEB_PREBUILT=1 VIDENOA_CONTROLLER_WEB_PREBUILT=1 cargo clippy --locked --workspace --all-targets -- -D warnings
cargo clippy --locked -p videnoa-controller --all-targets --all-features -- -D warnings
cargo test --locked -p videnoa-protocol
cargo test --locked -p videnoa-transport
cargo test --locked -p videnoa-core --lib --tests
cargo test --locked -p videnoa-app --lib --tests
cargo test --locked -p videnoa-controller --all-targets
cargo deny --locked check advisories licenses
node scripts/tests/validate_ci_release_workflows.test.mjs
```
