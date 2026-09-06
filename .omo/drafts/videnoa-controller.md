---
slug: videnoa-controller
status: complete
intent: clear
review_required: false
pending-action: choose `$start-work videnoa-controller` or request dual high-accuracy review
approach: Add a GPU-free controller workspace member with its own embedded React GUI, SQLite-backed durable orchestration, bounded and separately pooled transfers, crash-safe remote submission reconciliation, and independent Docker/release integration without renaming or coupling the existing Videnoa application.
---

# Draft: videnoa-controller

## Components (topology ledger)
<!-- Lock the SHAPE before depth. One row per top-level component that can succeed or fail independently. -->
<!-- id | outcome (one line) | status: active|deferred | evidence path -->

| id | outcome | status | evidence path |
|---|---|---|---|
| C1 | A GPU-free `videnoa-controller` service exposes its API, persists configuration/tasks/events in SQLite, and serves its GUI. | active | `Cargo.toml:1-35`; `crates/core/Cargo.toml:6-35`; `crates/core/src/server/mod.rs:542-624` |
| C2 | A typed Videnoa client manages worker discovery, workflow compatibility, polling, cancellation, and separately bounded upload/download streams. | active | `crates/core/src/server/mod.rs:420-450,1252-1923`; `crates/core/src/server/files.rs:34-177` |
| C3 | A durable state machine reconciles crashes and restarts without duplicate AI execution, including the submit/persist uncertainty window. | active | `crates/core/src/server/persistence.rs:64-216`; `.omo/knowledges/videnoa-controller-repo-findings.md:34-39` |
| C4 | A scheduler dispatches exactly the requested task ingress modes across multiple Videnoa instances without pre-uploading the queue. | active | user requirements; `.omo/knowledges/videnoa-controller-repo-findings.md:10-25` |
| C5 | A dense React/TypeScript Controller GUI operates workers, tasks, progress, failures, and settings from the Controller API. | active | `web/package.json:6-59`; `crates/core/build.rs:10-80`; `crates/core/src/server/mod.rs:542-569` |
| C6 | CI, packaging, Docker, release, documentation, and operational configuration deliver Controller independently from the existing GPU application. | active | `.github/workflows/unittest.yaml:19-225`; `.github/workflows/release.yaml:22-147,323-363`; `Dockerfile:39-62`; `scripts/package_dist.sh:70-96,207-251,410-465` |

## Open assumptions (announced defaults)
<!-- Record any default you adopt instead of asking, so the user can veto it at the gate. -->
<!-- assumption | adopted default | rationale | reversible? -->

| assumption | adopted default | rationale | reversible? |
|---|---|---|---|
| Controller persistence | SQLite is the sole durable source of truth; channels/notifiers only wake workers. | Required by the request and supports deterministic restart recovery. | no |
| Remote workflow discovery | Merge `/api/workflows` and `/api/presets`, then require compatible `Path` interfaces named `input` and `output`. | `/api/run` accepts both namespaces, but the list endpoints are separate. | yes |
| Transfer integrity | Upload/download by streaming; downloads land in temp `.part`, are length-checked and synced, then published with a cross-filesystem-safe move. | Prevents partial files from appearing as completed media. | yes |
| Remote cleanup | Treat remote DELETE 404 as idempotent success; successful tasks are not complete until local temp and remote workspace cleanup finish. | Supports retryable cleanup without rerunning AI work. | yes |
| Frontend integration | Give Controller its own frontend directory and release-build embedding flow modeled on the existing Vite + `rust-embed` setup. | Keeps Controller independently buildable while matching repository conventions. | yes |

## Findings (cited - path:lines)

- The workspace has three current members; GPU-heavy ONNX/CUDA/TensorRT dependencies are isolated in `videnoa-core`, so Controller can remain GPU-free by not depending on it (`Cargo.toml:1-35`; `crates/core/Cargo.toml:6-35`).
- Existing release Rust builds invoke npm and build the current frontend automatically from `crates/core/build.rs:10-80`; Controller needs an analogous isolated build path rather than extending the GPU core build script.
- Videnoa already exposes run, jobs, workflow/preset, and workspace file APIs sufficient for orchestration (`crates/core/src/server/mod.rs:577-624`).
- `/api/workflows` omits presets although `/api/run` resolves both; compatibility discovery must merge both sources and validate the `input`/`output` interface (`crates/core/src/server/mod.rs:1689-1923`).
- Videnoa restart marks queued/running remote work cancelled instead of resuming it; Controller recovery must reconcile rather than blindly resubmit (`crates/core/src/server/persistence.rs:156-165`).
- Remote file paths are workspace-relative API values and may contain `..` in returned workflow paths; they must never be interpreted as Controller-local paths (`crates/core/src/server/tests/files/mod.rs:65-84`).
- Current CI/release/package scripts know only the existing web app and binaries, while the Docker cache stage enumerates workspace manifests; all require explicit Controller integration (`.github/workflows/unittest.yaml:19-225`; `.github/workflows/release.yaml:22-147,323-363`; `Dockerfile:39-62`).

## Decisions (with rationale)

- Use a new workspace member under `crates/controller/` producing `videnoa-controller`; do not rename the existing application to "worker" and do not depend on `videnoa-core`.
- Persist a durable `SUBMITTING`-equivalent state before calling remote `/api/run`. On restart, search remote jobs using task-unique input/output paths before deciding whether submission is absent; ambiguity fails safely and never triggers blind recomputation.
- Use separate upload and download concurrency pools, plus independent AI-slot scheduling per configured Videnoa instance; do not pre-upload the full queue.
- Retry only the failed pipeline stage where safe. Cleanup/download/verification failures must not rerun completed AI processing.
- Keep the existing Videnoa API intact unless implementation proves a narrowly scoped additive endpoint is necessary to make reconciliation unambiguous; any such addition must remain backward-compatible.
- Use one administrator secret for both access modes: Web UI login verifies the admin password and issues a hardened server-side session, while programmatic API clients provide that same secret in an HTTP authentication header. Store/compare only a password hash, never place the secret in URLs or logs, rate-limit failed authentication, protect cookie-authenticated mutations against CSRF, and support secret-file injection for container deployments.
- Use the standard `Authorization: Bearer <admin-secret>` contract for programmatic API access. The Web UI exchanges the same secret for an `HttpOnly`, `SameSite=Strict` session cookie; it does not retain the raw secret in browser storage.
- Enforce canonical, configured `input_roots` and `output_roots` allowlists for all NAS paths regardless of authentication; authentication never grants access outside those roots.
- Reject/no-clobber when the final `output_path` already exists. Preserve the existing media and require a different output path before retrying; do not add silent renaming or an overwrite flag.
- Use TDD: each behavior-changing implementation todo begins with a failing test, followed by the minimal implementation and the relevant regression suite. Agent-executed happy/failure QA and the final verification wave remain mandatory.
- Publish `controlnet/videnoa-controller:<version>` and `controlnet/videnoa-controller:latest` using the existing Docker Hub credentials/conventions, plus separate Linux and Windows Controller archives in the existing GitHub Release workflow. Do not add Controller to the current GPU image or existing Videnoa archives.
- Manual UI creation and external automation use exactly the same `POST /api/tasks`; `source` remains informational and no ANI-RSS-specific backend logic is added (`.omo/knowledges/videnoa-controller-request.md`).
- Preserve task-local path semantics and extensions exactly: immutable NAS `input_path`, caller-selected NAS `output_path`, and task-ID remote workspaces with independently derived input/output suffixes (`.omo/knowledges/videnoa-controller-request.md`).
- Use default `compute_slots = 1`, `prefetch_per_worker = 1`, `max_concurrent_uploads = 1`, and `max_concurrent_downloads = 1`, all configurable. Idle-worker feed uploads outrank optional prefetch uploads.
- Verify downloaded artifacts using recorded HTTP length/local file length and non-zero size before no-clobber publication. Do not make `ffprobe` a mandatory Controller runtime dependency in the initial implementation.

## Scope IN

- Controller Rust service/API, SQLite schema and migrations, configuration, task/event persistence, startup reconciliation, graceful shutdown, health/readiness, and structured logs.
- Multi-instance Videnoa registry and capability checks; workflow/preset discovery; compatible interface validation; run/poll/cancel/file API client.
- Exactly the two requested task-ingress paths, durable queueing, scheduler fairness/capacity, transfer pools, progress/error tracking, targeted retries, and cleanup.
- Safe local input/output path handling, temp staging, integrity verification, no partial publication, and explicit collision behavior once selected below.
- Controller-specific React/TypeScript/Tailwind GUI served by the Controller binary.
- Unit/integration/e2e tests, CI, independent Docker image/release packaging, configuration examples, operator documentation, and migration/rollback notes.

## Scope OUT (Must NOT have)

- Must not rename the existing Videnoa application or rewrite it as a worker product.
- Must not link Controller to GPU-heavy `videnoa-core`, CUDA, TensorRT, ONNX Runtime, or model execution code.
- Must not replace SQLite as the durable source of truth with an in-memory queue or external broker.
- Must not pre-upload the entire queue, combine upload/download limits into one pool, or download directly into the final media path.
- Must not silently overwrite media, trust arbitrary filesystem paths, or expose unauthenticated control endpoints beyond the selected deployment boundary.
- Must not rerun AI processing merely because download, verification, publication, or cleanup failed later.
- Must not fold Controller into the current GPU Docker image or existing binary archive unless the user explicitly chooses that distribution model.

## Open questions

None. All repository-discoverable facts and owner decisions required for a decision-complete plan are resolved.

## Approval gate
status: approved
<!-- When exploration is exhausted and unknowns are answered, set status: awaiting-approval. -->
<!-- That durable record is the loop guard: on a later turn read it and resume at the gate instead of re-running exploration. -->

### Approval brief state

- Proposed plan: six components covering the GPU-free Controller service/persistence, remote API and transfer client, crash-safe lifecycle/reconciliation, multi-worker scheduler, dense GUI, and independent delivery/operations.
- Owner decisions resolved: one admin secret for login and Bearer API authentication; canonical NAS root allowlists; output no-clobber; TDD; independent Docker plus GitHub binaries.
- Next action after explicit approval: create `.omo/plans/videnoa-controller.md`, run mandatory Metis gap analysis, append the complete task/QA/commit waves, validate the plan structure, then offer either `$start-work videnoa-controller` or optional dual high-accuracy review.
- Approval received from the project owner. Plan creation is authorized; product-code execution remains unauthorized in this planning session.
- Final plan written at `.omo/plans/videnoa-controller.md` with 25 implementation todos and 4 final-verification tasks.
- Mandatory Metis gap analysis session `ses_fa22ee6aaffeY2x6FCHGcf3xfj` identified unsafe submission, publication, cancellation, auth, path, outage, and delivery gaps; all were resolved into locked contracts and executable QA obligations in the final plan.
- Structural self-check passed: required heading order is intact; implementation rows are column-zero `- [ ] N.` for 1-25; final rows are column-zero `- [ ] F1.` through `F4.`; no placeholder task substitutes remain.
