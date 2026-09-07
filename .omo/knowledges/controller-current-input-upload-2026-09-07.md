# Upload current input and retry historical input changes

## Requested behavior

The user explicitly removed the requirement to reject source changes between task creation and upload. This supersedes the admission-snapshot comparison requirements in `controller-input-content-identity.md`, the upload-hashing portion of `controller-input-scans-transfer-inactivity-2026-09-07.md`, and the new-task workaround in `controller-input-changed-relay-diagnosis-2026-09-07.md`.

- Fresh uploads open the current regular file through the existing media path capabilities. They do not hash it or compare its content, filesystem identity, size, or mtime with the task's intake snapshot.
- If the current size differs, persist it using the task version and uploading-state CAS before PUT. Use the updated version for subsequent lifecycle writes. Upload framing, receipt/stat checks, and crash recovery therefore use the current upload size.
- Retained-root checks, private-storage boundaries, regular-file requirements, and transfer-size verification remain. Missing or disallowed input fails with `input_unavailable` and an accurate message.
- Intake still records its existing snapshot and performs one content hash; no schema migration or broad intake refactor was introduced. Upload performs zero content hashes. A source changed during the actual transfer is not an immutable snapshot and may still cause a transfer failure.
- Historical `input_changed` failures at the upload stage support explicit manual retry even when their stored `retryable` flag is false. Both backend classification and frontend eligibility allow this exception. Existing failed tasks are not automatically restarted or edited in the database.
- Retry keeps the task path and attempt, resumes uploading, and accepts the current file. Other failure/stage combinations retain their existing retry policy. Task details report actual manual-retry availability rather than the historical flag alone.
- Deploy the updated Controller backend and frontend together. NAS deployment was not performed during this source change.

## Verification

Regression fixtures use explicitly synthetic test-only media bytes. Coverage includes replacement with a larger file, changed bytes with identical metadata, a smaller replacement without upload hashing, missing input, historical terminal failures through API and upload execution, and recovery after successful PUT followed by a stat failure. The mock Worker stat endpoint now honors its existing scripted response-fault mechanism for the recovery test.

Run from the repository root; tests must pass, Clippy must report no warnings, and formatting/docs checks must exit zero:

```bash
CARGO_TARGET_DIR=/tmp/videnoa-iroh-target cargo test --locked -p videnoa-controller --lib --test lifecycle --test task12 --test task14 --test path_capabilities --test workspace_paths
CARGO_TARGET_DIR=/tmp/videnoa-iroh-target cargo clippy --locked -p videnoa-controller --all-targets --all-features -- -D warnings
npm --prefix controller-web test -- src/tasks
npm --prefix controller-web run lint
npm --prefix controller-web run build
bash scripts/tests/controller_docs_test.sh
git diff --check
```

Changed Rust modules also require direct `rustfmt --check --edition 2021` verification because the Controller library's include-based module topology is not fully traversed by workspace cargo-fmt.
