# Restricted File Transfer API

## Architecture

- Controller-facing transfers use `/api/files/*`; trusted Web UI browsing remains on unchanged `/api/fs/*` handlers.
- `AppState` derives and creates `<data_dir>/workspace` during construction.
- Production handlers live in `crates/core/src/server/files.rs`; the shared containment boundary lives in `crates/core/src/server/files/path.rs`.
- Upload uses Axum `Body::into_data_stream`, `StreamReader`, and `tokio::io::copy`; download uses `ReaderStream` and `Body::from_stream` with `Content-Length`.
- Metadata is `GET /api/files/{path}/stat`; the final `/stat` suffix is reserved because Axum catch-all parameters must terminate the route.

## Security Boundary

- Only UTF-8 workspace-relative paths are accepted.
- Empty, absolute, Windows-drive, backslash, tilde-leading, `.` and `..` paths are rejected with 400.
- Every existing component is checked with `symlink_metadata`; symlinks are rejected and intermediate components must be directories.
- Existing targets or the nearest existing upload ancestor are canonicalized and required to stay under the canonical workspace root.
- Upload validation is repeated after parent creation; Unix upload opens use `O_NOFOLLOW` for the final target.
- Workspace-root deletion is rejected on both `/api/files` and `/api/files/`.

## Verification

- RED: 11 lifecycle/security tests failed on the absent routes or absent workspace.
- GREEN: 12 targeted tests passed; the full `videnoa-core` suite passed with 590 tests, 10 ignored, plus 3 integration tests.
- `cargo clippy -p videnoa-core --all-targets --all-features -- -D warnings` and `cargo build --workspace` passed.
- Live HTTP QA streamed a 4 MiB file through PUT/GET, verified stat and byte equality, rejected traversal/absolute/root/symlink attacks, preserved the outside symlink target, kept `/api/fs/browse` accessible, and recursively deleted the task directory.

## Scope Note

The API prevents remote-controller filesystem escape. A hostile local process racing filesystem component replacement remains outside the remote-only threat model and would require descriptor-relative platform APIs for complete TOCTOU resistance.
