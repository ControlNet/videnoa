# Videnoa Controller planning findings

## Repository and dependency boundaries

- The Cargo workspace currently contains `crates/core`, `crates/app`, and `crates/desktop`; the workspace version is `0.1.2` (`Cargo.toml:1-7`).
- GPU-heavy dependencies (`ort` with CUDA/TensorRT, `ndarray`, `half`) are consumed by `videnoa-core` (`Cargo.toml:8-35`, `crates/core/Cargo.toml:6-35`). `videnoa-controller` can remain GPU-free by depending directly on Axum/Tokio/Serde/reqwest/SQLite crates and not on `videnoa-core`.
- Existing web assets are built separately with npm/Vite and embedded into release Rust binaries with `rust-embed`; debug builds serve `web/dist` from disk (`crates/core/src/server/mod.rs:542-569`, `crates/app/src/lib.rs:294-313`).
- Release builds of `videnoa-core` automatically run `npm ci`/`npm install` and `npm run build` for the existing `web/` frontend from the crate build script (`crates/core/build.rs:10-80`). A separate Controller frontend should mirror this convention without coupling its build to the GPU core crate.
- The existing frontend stack is npm + React 19 + TypeScript 5.9 + Vite 7 + Tailwind 4 + Radix + Lucide + React Router + Zustand + Vitest (`web/package.json:6-59`).

## Existing Videnoa remote API contracts

- Routes needed by Controller already exist: health, run, jobs, workflows, presets, and workspace-scoped files (`crates/core/src/server/mod.rs:577-624`).
- `POST /api/run` accepts only `workflow_name` and optional arbitrary JSON `params`, rejects unknown top-level fields, resolves `<name>.json` from workflows before presets, and returns HTTP 201 with `{id,status,created_at}` (`crates/core/src/server/mod.rs:420-450`, `1252-1407`; tests `3548-3775`).
- `GET /api/jobs/{id}` returns status, timestamps, progress (`current_frame`, `total_frames`, `fps`, `eta_seconds`), error, workflow metadata, params, and duration; missing jobs return 404 (`crates/core/src/server/mod.rs:282-306`, `437-450`, `1498-1519`, `2540-2582`).
- `DELETE /api/jobs/{id}` cancels queued/running execution through its cancellation token and removes the remote history row; it is therefore usable for explicit Controller cancellation, not ordinary polling (`crates/core/src/server/mod.rs:1558-1604`; tests around `4071-4101`).
- Remote Videnoa persists jobs in SQLite, but on its own restart converts queued/running jobs to cancelled rather than resuming compute (`crates/core/src/server/persistence.rs:64-216`, especially `156-165`; tests `4405-4481`). Controller reconciliation must treat this as a terminal remote failure and never blindly duplicate compute.
- `GET /api/workflows` lists only saved workflow files, while `GET /api/presets` lists built-in presets; `/api/run` can resolve either. Controller workflow availability must therefore merge both sources. `GET /api/workflows/<name>.json/interface` checks workflows first and then presets (`crates/core/src/server/mod.rs:1689-1750`, `1750-1796`, `1890-1923`).
- Existing shipped workflows expose required `Path` inputs named `input` and `output` (`presets/anime-2x-upscale.json:5-96`, `presets/interpolation-3x-anime-2x.json:5-113`). Controller should validate this contract per worker rather than assume every named workflow is compatible.

## File API details

- `PUT /api/files/<relative>` streams the request body to `<data_dir>/workspace/<relative>`, truncates an existing file, creates parent directories, and returns a process-CWD-relative workflow path plus byte size (`crates/core/src/server/files.rs:34-100`).
- `GET /api/files/<relative>` streams bytes with `Content-Length`; `GET .../stat` returns workflow path, size, and file/dir flags; `DELETE` removes a file or directory recursively (`crates/core/src/server/files.rs:103-177`; tests `crates/core/src/server/tests/files/lifecycle.rs:12-122`).
- File paths reject traversal, absolute/home/drive-prefixed paths, symlinks, and workspace-root deletion (`crates/core/src/server/files/path.rs:22-191`; `crates/core/src/server/tests/files/security.rs:3-89`).
- Upload responses may contain `..` components when the worker data directory is outside its process CWD; Controller must persist the exact returned input workflow path and derive the sibling output path without interpreting it as a Controller-local filesystem path (`crates/core/src/server/tests/files/mod.rs:65-84`).

## Existing release conventions and integration hazards

- CI runs Rust tests on Linux/Windows, builds the existing web app with Node 20/npm, performs Linux/Windows package smoke checks, and builds the GPU Docker image (`.github/workflows/unittest.yaml:19-225`).
- Releases are workspace-version gated, publish GitHub archives, and push `controlnet/videnoa:<version>` plus `latest` to Docker Hub (`.github/workflows/release.yaml:22-147`, `323-363`).
- The current Dockerfile copies every workspace member manifest before dependency caching. Adding a new workspace member requires updating these early COPY/dummy-source steps even when building only `videnoa` (`Dockerfile:39-62`).
- Existing package scripts build only the root `web/` frontend and validate a bundle containing only `videnoa` and `videnoa-desktop`; a Controller frontend/binary requires an explicit packaging decision rather than silently altering the current GPU bundle (`scripts/package_dist.sh:70-96`, `207-251`, `410-465`; PowerShell equivalent in `scripts/package_dist.ps1`).

## Planning implications already established

- SQLite must remain the queue/source of truth; runtime channels/notifiers are wake-up mechanisms only.
- A durable pre-submit/submitting marker is needed to close the crash window between calling remote `/api/run` and persisting `remote_job_id`. Recovery can search remote jobs by the task-unique input/output workspace paths before deciding that submission is missing; ambiguity must never trigger blind resubmission.
- Downloads should stream to a Controller temp `.part`, sync, validate expected length, and rename within temp before verification. Publishing then needs a no-clobber policy and a cross-filesystem fallback that never downloads directly into the media library.
- Successful completion is gated on local temp cleanup and idempotent remote workspace deletion; remote DELETE 404 should be treated as already-cleaned success.
