# Videnoa Controller Worker Health Runtime

## Composition invariant

Worker registration must not claim online state. The production composition root owns a long-lived `WorkerHealthService` beside `Orchestrator`; only successful typed remote probes make a worker online and compatible.

## Durable scheduling wake

Health refresh persists through `WorkerRegistry::refresh_health`. That write emits `DurableChange::Worker` through the existing observer, so orchestration rescans queued work. Capability cache contents are an optimization only; scheduler compatibility remains derived from persisted worker capabilities.

## Failure contract

Probe failures mark the worker offline and persist bounded exponential backoff from runtime retry settings. Existing capabilities and `last_seen_at` remain durable evidence. Health and remote failures invalidate the in-memory capability cache so recovery performs fresh discovery.

## Test fixture rule

Production-shaped worker setup must use the authenticated worker API and wait for durable online state. Tests that intentionally exercise initial probe failure use the same API through a no-wait helper; they must never inject `WorkerHealthUpdate` directly.

## Module boundary

Keep runtime scanning and persistence in `workers/health.rs` and remote probe classification in `workers/health/probe.rs`. This preserves the Controller's 250 pure-LOC file ceiling without splitting durable policy from the service that owns it.
