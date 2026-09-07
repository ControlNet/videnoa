# Controller Jellyfin suffix implementation

The batch naming mode `jellyfin_version_suffix` generates `<stem> - <label>.<ext>`.
The API retains `middle_extension` for the label, preserving historical request
serialization and durable idempotency fingerprints. Existing defaults remain
`insert_extension` and `AI`. Preview and HTTP batch creation use the same backend
path generation. No worker changes or database migration are required.

The Add Batch dialog exposes `Jellyfin version suffix` and `Version Label` with a
live example. Switching output location preserves a compatible naming mode. Only
`original` falls back to `insert_extension` when switching beside the input.
Labels reject surrounding whitespace, leading/trailing dots, invalid filename
characters, control characters, and lengths outside 1–64 UTF-8 bytes. Existing
middle-extension validation remains compatible. Naming appends to the complete
stem, preserving dots, Unicode, existing suffixes, and extension case. Existing
output and duplicate-destination checks still block unsafe submission.

See `controller-jellyfin-version-suffix-2026-09-07.md` for the upstream Jellyfin 12
investigation. No live Jellyfin scan or GPU processing was performed here.

## Verification

Tests use explicitly synthetic temporary intake files and mocked browser API
responses; they do not submit real media jobs. The new API tests failed before
implementation because the server rejected the new naming mode, then passed.
Controller library and task API suites passed (33 and 43 tests), including the
historical fingerprint regression. The documentation Rust target also passed.
Frontend tests passed 147 cases; all seven batch browser tests passed, including
new desktop/mobile submission and accessibility checks. Screenshots at
`.omo/evidence/controller-batch-tasks/jellyfin-1280.png` and `jellyfin-375.png`
were visually inspected. Build warnings about Zod pure annotations are nonfatal.
Strict Clippy required replacing a manually constructed empty test string with
`String::new()`.

Run from the repository root:

```bash
cargo test --locked -p videnoa-controller --lib --test task_api --test controller_docs
cargo clippy --locked -p videnoa-controller --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
rustfmt --edition 2021 --check crates/controller/src/tasks/batch.rs crates/controller/tests/task_api/batch.rs
bash scripts/tests/controller_docs_test.sh
npm --prefix controller-web run lint
npm --prefix controller-web test
npm --prefix controller-web run build
(cd controller-web && npx playwright test tests/e2e/batch-tasks.spec.ts)
git diff --check
```

Expected: all commands exit zero, no failed tests, no Clippy or formatting errors,
and documentation checks report PASS. For a manual application check, select
the new format, leave label `AI`, and preview `Re Zero S03E01.mkv`; the output
must be `Re Zero S03E01 - AI.mkv` in the selected destination.
