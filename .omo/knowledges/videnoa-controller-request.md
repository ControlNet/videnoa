# Videnoa Controller explicit product requirements

Source: original project-owner request in OpenCode session `ses_fa2500002ffeEX1aBzRDiIy1q8` on 2026-09-01.

## Stable product boundary

- Add `crates/controller/` and the independently runnable `videnoa-controller` binary inside the existing repository.
- Keep the existing GPU service named `videnoa`; do not rename it to worker and do not make Controller depend on GPU-heavy `videnoa-core`.
- Primary deployment is a NAS beside ANI-RSS and Jellyfin storage. Remote `videnoa` instances are reached over HTTP/HTTPS, commonly Tailscale. Do not require SSH, SFTP, rsync, network mounts, or object storage.
- The stable abstraction is `Task(input_path, output_path, workflow)`, where both paths are Controller/NAS-local and remote execution is an implementation detail.

## Exact intake and path semantics

- Tasks enter in exactly two ways: manual creation in the Controller Web UI and external creation through the Controller HTTP API. Both call the same `POST /api/tasks` backend.
- Do not add watchers, directory polling, ANI-RSS/qBittorrent adapters, cron discovery, or a rules engine. `source` is informational metadata only.
- `input_path` is immutable and read-only. The caller explicitly supplies the exact `output_path`; input/output extensions may differ and must never be guessed or forced to match.
- Each task uses a remote `<task-id>/input.<input-ext>` and `<task-id>/output.<output-ext>` workspace.

## Durable lifecycle

- SQLite is authoritative for queued work, assignments, remote job IDs, attempts, failures, completed history, and restart recovery. Runtime channels/locks/semaphores are coordination only.
- Preserve stage semantics: queued, reserved, uploading, staged, submitting/processing, remote-completed, downloading, verifying, publishing, remote-cleanup, completed, failed, cancelled.
- Later-stage failure must resume/retry that stage and must not repeat earlier expensive AI work. Publishing is followed by local temp cleanup and mandatory remote workspace deletion before completed.
- Retain permanent task and attempt history. Store `failure_stage` and error. Reconciliation is distinct from ordinary scheduling.

## Scheduling and transfer invariants

- A worker is one remotely callable Videnoa service instance with configurable `compute_slots`, default 1. Controller must not schedule GPUs/devices/VRAM directly.
- Worker eligibility requires online/enabled status and the named compatible workflow. Do not synchronize workflows.
- Default `prefetch_per_worker = 1`; do not stage/upload the whole queue.
- Upload and download use separate global concurrency pools, default 1 each. Per worker, allow at most one active upload and one prefetched task initially while compute and download may overlap.
- Feeding an idle worker has upload priority over optional prefetch. Scheduler pause stops new reservation/prefetch/compute starts but does not kill running processing and may allow downloads/cleanup to finish.

## Storage and history scale

- Download into Controller-owned temp storage outside Jellyfin, never into the final media directory. Verify before publishing to the exact requested output path.
- Expected retained history is thousands to tens of thousands of tasks. Task APIs/UI require indexed server pagination, filtering, search, and sorting; never return/load all history.
- Initial entities should remain focused: `tasks`, `workers`, `task_attempts`, and `controller_settings`, plus only narrowly justified supporting records/migrations.

## GUI requirements

- Controller has a separate React/TypeScript/Tailwind GUI. Browser calls only Controller; Controller calls Videnoa.
- Main pages: Tasks, Workers, Settings. Tasks is a dense qBittorrent/Transmission-style paginated table, not a card-heavy dashboard.
- Provide compact task creation, detailed bottom-pane/task view, attempts/errors/progress, worker management, server-side filters/sort/search, and efficient live updates for active rows only.

## Explicit non-goals

- No distributed consensus, Redis/PostgreSQL requirement, brokers/Kafka, Kubernetes-style scheduling, native GPU scheduling, workflow deployment, resumable upload protocol, complex accounts/ACLs, automatic history deletion, sophisticated media browser, or complex automation system.
- Keep the architecture robust, understandable, modular, and testable without unnecessary distributed-system abstractions.

## Owner decisions added during planning

- One admin password protects both access modes: Web login creates a hardened session; programmatic clients authenticate using the same secret in an HTTP header.
- All NAS paths remain confined to configured canonical input/output allowlists.
- Existing final output causes a no-clobber failure; never silently overwrite or auto-rename.
- Test strategy is TDD plus agent-executed QA.
- Publish an independent `controlnet/videnoa-controller` Docker image and independent Linux/Windows Controller archives while leaving existing `videnoa` artifacts unchanged.
