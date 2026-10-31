import { describe, expect, it } from "vitest"

import type { Task } from "../api/taskSchemas"
import {
  canAbandonTask,
  canCancelTask,
  canDiscardTask,
  canRetryOnAnotherWorker,
  canRetryTask,
  failureGuidance,
} from "./taskActionPolicy"

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
  requested_worker_id: null,
  remote_job_id: null,
  progress: { percent: 0, processed_frames: null, total_frames: null, frames_per_second: null, eta_seconds: null, bytes_transferred: null, bytes_total: null },
  attempt_count: 1,
  failure: null,
  cancel_requested_at: null,
  created_at: "2026-09-02T00:00:00Z",
  updated_at: "2026-09-02T00:00:00Z",
  completed_at: null,
  allowed_actions: [],
} as const satisfies Task

const offered = (candidate: Task) => ({
  cancel: canCancelTask(candidate),
  abandon: canAbandonTask(candidate),
  retry: canRetryTask(candidate),
  retryOnAnotherWorker: canRetryOnAnotherWorker(candidate),
  discard: canDiscardTask(candidate),
})

describe("task lifecycle action policy", () => {
  it("offers exactly the actions the Controller allows", () => {
    // Given: a failed task whose failure looks retryable, and the Controller's verdicts.
    const failed: Task = {
      ...task,
      status: "failed",
      failure: { failure_stage: "processing", failure_code: "processing_failed", message: "failed", retryable: true },
      allowed_actions: ["retry", "retry_on_another_worker", "discard"],
    }

    // When/Then: the UI follows the list, not its own reading of the failure.
    expect(offered(failed)).toEqual({ cancel: false, abandon: false, retry: true, retryOnAnotherWorker: true, discard: true })
    expect(offered({ ...failed, allowed_actions: ["discard"] })).toEqual({
      cancel: false,
      abandon: false,
      retry: false,
      retryOnAnotherWorker: false,
      discard: true,
    })
    expect(offered({ ...task, allowed_actions: ["cancel"] }).cancel).toBe(true)
    expect(offered({ ...task, cancel_requested_at: "2026-09-02T00:06:00Z", allowed_actions: ["abandon"] })).toEqual({
      cancel: false,
      abandon: true,
      retry: false,
      retryOnAnotherWorker: false,
      discard: false,
    })
  })

  it("offers nothing the Controller did not list, including actions it does not know", () => {
    // Given: a task with no allowed actions, and one with only a future action.
    const none = offered(task)
    const future = offered({ ...task, allowed_actions: ["rewind"] })

    // Then: no button is offered for either.
    expect(Object.values(none).some(Boolean)).toBe(false)
    expect(Object.values(future).some(Boolean)).toBe(false)
  })

  it("explains manual retries of failures recorded as nonretryable", () => {
    expect(failureGuidance("input_changed", "upload").kind).toBe("stage_retry")
    expect(failureGuidance("input_changed", "processing").kind).toBe("blocked")
    expect(failureGuidance("remote_submission_failed", "submission").kind).toBe("stage_retry")
    expect(failureGuidance("remote_submission_failed", "processing").kind).toBe("blocked")
  })

  it("routes collision and processing guidance without changing task paths", () => {
    // Given/When: the operator asks what each recovery action means.
    const collision = failureGuidance("output_exists", "publication")
    const processing = failureGuidance("processing_failed", "processing")

    // Then: path changes create a new task while processing retry verifies remote state.
    expect(collision.kind).toBe("new_task")
    expect(processing.kind).toBe("processing_retry")
    expect(failureGuidance("publication_ambiguous", "publication").kind).toBe("stage_retry")
  })
})
