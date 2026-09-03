import { describe, expect, it } from "vitest"

import type { Task } from "../api/taskSchemas"
import { canCancelTask, canRetryTask, failureGuidance } from "./taskActionPolicy"

const task = {
  id: "00000000-0000-4000-8000-000000000001",
  version: 4,
  status: "processing",
  input_path: "/nas/input/exact.mkv",
  output_path: "/nas/output/exact.mp4",
  input_extension: "mkv",
  output_extension: "mp4",
  workflow: "anime-2x",
  priority: 17,
  source: "manual",
  source_reference: null,
  input_size: 1024,
  worker_id: null,
  remote_job_id: null,
  progress: { percent: 0, processed_frames: null, total_frames: null, frames_per_second: null, eta_seconds: null, bytes_transferred: null, bytes_total: null },
  attempt_count: 1,
  failure: null,
  cancel_requested_at: null,
  created_at: "2026-09-02T00:00:00Z",
  updated_at: "2026-09-02T00:00:00Z",
  completed_at: null,
} as const satisfies Task

describe("task lifecycle action policy", () => {
  it("allows cancellation only from queued through verifying", () => {
    // Given/When: every boundary status is evaluated.
    const allowed = ["queued", "reserved", "uploading", "staged", "submitting", "processing", "remote_completed", "downloading", "verifying"] as const
    const blocked = ["publishing", "remote_cleanup", "completed", "failed", "cancelled"] as const

    // Then: the UI matches the backend cancellation window.
    expect(allowed.every((status) => canCancelTask({ ...task, status }))).toBe(true)
    expect(blocked.every((status) => !canCancelTask({ ...task, status }))).toBe(true)
  })

  it("blocks cancellation after cancellation intent is persisted", () => {
    // Given: an otherwise cancellable processing task with durable cancellation intent.
    const cancelling = { ...task, cancel_requested_at: "2026-09-02T00:06:00Z" }

    // When: action eligibility is derived from authoritative detail.
    const allowed = canCancelTask(cancelling)

    // Then: the UI cannot issue a repeated cancellation request.
    expect(allowed).toBe(false)
  })

  it("requires explicit retryable evidence and blocks ambiguity", () => {
    // Given: retryable processing evidence and contradictory ambiguity evidence.
    const processing = {
      ...task,
      status: "failed",
      failure: { failure_stage: "processing", failure_code: "processing_failed", message: "failed", retryable: true },
    } as const
    const ambiguous = { ...processing, failure: { ...processing.failure, failure_code: "remote_state_ambiguous" } } as const

    // When/Then: retry is enabled only for explicit safe evidence.
    expect(canRetryTask(processing)).toBe(true)
    expect(canRetryTask(ambiguous)).toBe(false)
    expect(canRetryTask({ ...processing, failure: { ...processing.failure, retryable: false } })).toBe(false)
  })

  it.each([
    ["processing_failed", "processing"],
    ["transfer_failed", "upload"],
    ["transfer_failed", "download"],
    ["verification_failed", "verification"],
    ["publication_failed", "publication"],
    ["cleanup_failed", "local_cleanup"],
    ["cleanup_failed", "remote_cleanup"],
  ] as const)("allows the Rust-supported %s and %s retry pair", (failureCode, failureStage) => {
    // Given: a failed task with explicit retryability and one supported code/stage pair.
    const retryable = {
      ...task,
      status: "failed",
      failure: { failure_stage: failureStage, failure_code: failureCode, message: "failed", retryable: true },
    } as const

    // When/Then: the frontend mirrors the Rust retry classifier.
    expect(canRetryTask(retryable)).toBe(true)
  })

  it.each([
    ["processing_failed", "download"],
    ["transfer_failed", "processing"],
    ["verification_failed", "publication"],
    ["publication_failed", "verification"],
    ["cleanup_failed", "upload"],
  ] as const)("blocks the contradictory %s and %s retry pair", (failureCode, failureStage) => {
    // Given: explicit retryability attached to a Rust-unsupported code/stage pair.
    const contradictory = {
      ...task,
      status: "failed",
      failure: { failure_stage: failureStage, failure_code: failureCode, message: "failed", retryable: true },
    } as const

    // When/Then: contradictory persisted evidence remains blocked.
    expect(canRetryTask(contradictory)).toBe(false)
  })

  it.each(["remote_state_ambiguous", "publication_ambiguous"] as const)("blocks %s despite contradictory retryable metadata", (failureCode) => {
    // Given: ambiguity evidence incorrectly marked retryable.
    const ambiguous = {
      ...task,
      status: "failed",
      failure: { failure_stage: "processing", failure_code: failureCode, message: "ambiguous", retryable: true },
    } as const

    // When/Then: ambiguity outranks retryable metadata.
    expect(canRetryTask(ambiguous)).toBe(false)
  })

  it("routes collision and processing guidance without changing task paths", () => {
    // Given/When: the operator asks what each recovery action means.
    const collision = failureGuidance("output_exists", "publication")
    const processing = failureGuidance("processing_failed", "processing")

    // Then: path changes create a new task while processing retry verifies remote state.
    expect(collision.kind).toBe("new_task")
    expect(processing.kind).toBe("processing_retry")
  })
})
