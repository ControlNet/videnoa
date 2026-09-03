import { describe, expect, it } from "vitest"

import { cancelTaskResponseSchema, retryTaskResponseSchema, taskCreateRequestSchema, taskDetailSchema } from "./taskSchemas"

const progress = {
  percent: 37.5,
  processed_frames: 900,
  total_frames: 2400,
  frames_per_second: 12.5,
  eta_seconds: 120,
  bytes_transferred: 1_048_576,
  bytes_total: 4_194_304,
} as const

const task = {
  id: "00000000-0000-4000-8000-000000000001",
  version: 4,
  status: "processing",
  input_path: "/nas/input/Season ../episode.v1.mkv",
  output_path: "/nas/output/Season ../episode.final.mp4",
  input_extension: "mkv",
  output_extension: "mp4",
  workflow: "anime upscale ../v2",
  priority: 17,
  source: "manual",
  source_reference: null,
  input_size: 4_194_304,
  worker_id: "00000000-0000-4000-8000-000000000003",
  remote_job_id: "00000000-0000-4000-8000-000000000005",
  progress,
  attempt_count: 1,
  failure: null,
  cancel_requested_at: null,
  created_at: "2026-09-02T00:00:00Z",
  updated_at: "2026-09-02T00:05:00Z",
  completed_at: null,
} as const

describe("Task 17 API schemas", () => {
  it("parses the exact manual create request without changing paths", () => {
    // Given: an operator-selected MKV input and MP4 output.
    const request = {
      input_path: "/nas/input/Show/episode.01.mkv",
      output_path: "/nas/output/Show/episode.01.mp4",
      workflow: "anime-2x",
      priority: 17,
      source: "manual",
      source_reference: null,
    } as const

    // When: the request crosses the form boundary.
    const parsed = taskCreateRequestSchema.parse(request)

    // Then: exact caller spelling and explicit manual source survive.
    expect(parsed).toEqual(request)
  })

  it("parses persisted attempts and rejects invented detail fields", () => {
    // Given: the authoritative task detail DTO returned by Rust.
    const detail = {
      task,
      attempts: [
        {
          id: "00000000-0000-4000-8000-000000000002",
          task_id: task.id,
          attempt_number: 1,
          worker_id: task.worker_id,
          status: "processing",
          submission_key: "00000000-0000-4000-8000-000000000004",
          remote_job_id: task.remote_job_id,
          remote_input_path: "task/input/../opaque.mkv",
          remote_output_path: "task/output/../opaque.mp4",
          progress,
          retry: { retry_count: 0, next_retry_at: null },
          failure: null,
          created_at: "2026-09-02T00:00:01Z",
          started_at: "2026-09-02T00:00:02Z",
          completed_at: null,
        },
      ],
      total: 1,
      limit: 100,
      offset: 0,
    }

    // When: detail is parsed with one unknown server field added.
    const parsed = taskDetailSchema.parse(detail)
    const unknown = taskDetailSchema.safeParse({ ...detail, logs: ["invented"] })

    // Then: persisted evidence is accepted and invented logs are rejected.
    expect(parsed.attempts[0]?.remote_input_path).toBe("task/input/../opaque.mkv")
    expect(unknown.success).toBe(false)
  })

  it("rejects a non-UUID submission key", () => {
    // Given: otherwise valid detail with a submission key Rust cannot serialize.
    const detail = {
      task,
      attempts: [
        {
          id: "00000000-0000-4000-8000-000000000002",
          task_id: task.id,
          attempt_number: 1,
          worker_id: task.worker_id,
          status: "processing",
          submission_key: "opaque-but-not-a-uuid",
          remote_job_id: task.remote_job_id,
          remote_input_path: null,
          remote_output_path: null,
          progress,
          retry: { retry_count: 0, next_retry_at: null },
          failure: null,
          created_at: "2026-09-02T00:00:01Z",
          started_at: null,
          completed_at: null,
        },
      ],
      total: 1,
      limit: 100,
      offset: 0,
    }

    // When: the response crosses the strict API boundary.
    const parsed = taskDetailSchema.safeParse(detail)

    // Then: the wire contract rejects the invalid identifier.
    expect(parsed.success).toBe(false)
  })

  it("parses optimistic cancel and retry responses exactly", () => {
    // Given/When: Rust response DTOs cross the boundary.
    const cancel = cancelTaskResponseSchema.parse({ task_id: task.id, status: "processing", cancel_requested_at: "2026-09-02T00:06:00Z" })
    const retry = retryTaskResponseSchema.parse({ task_id: task.id, attempt_id: "00000000-0000-4000-8000-000000000007", status: "queued" })

    // Then: action identities and lifecycle statuses remain typed.
    expect(cancel.task_id).toBe(task.id)
    expect(retry.status).toBe("queued")
  })
})
