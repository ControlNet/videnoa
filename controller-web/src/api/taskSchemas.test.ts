import { describe, expect, it } from "vitest"

import {
  cancelTaskResponseSchema,
  retryTaskResponseSchema,
  taskCreateRequestSchema,
  taskDetailSchema,
  taskListSchema,
  taskSchema,
  taskStatusCountsSchema,
} from "./taskSchemas"

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
  requested_worker_id: null,
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

  it("keeps the task rows it can read when a newer Controller sends one it cannot", () => {
    // Given: a page with one task in a status this UI does not know yet.
    const page = { items: [task, { ...task, id: "00000000-0000-4000-8000-000000000009", status: "paused" }], total: 2, limit: 100, offset: 0 }

    // When: the page crosses the boundary.
    const parsed = taskListSchema.parse(page)

    // Then: only the unreadable row is left out.
    expect(parsed.items.map((item) => item.id)).toEqual([task.id])
    expect(parsed.total).toBe(2)
  })

  it("keeps the status counts it can read", () => {
    const counts = taskStatusCountsSchema.parse({ items: [{ status: "queued", count: 2 }, { status: "paused", count: 1 }], total: 3 })

    expect(counts.items).toEqual([{ status: "queued", count: 2 }])
  })

  it("ignores response fields a newer Controller adds", () => {
    // Given: a task carrying a field this UI does not know.
    const parsed = taskListSchema.parse({ items: [{ ...task, added_later: true }], total: 1, limit: 100, offset: 0 })

    // Then: the task is read and the unknown field is dropped.
    expect(parsed.items[0]).toEqual(taskSchema.parse(task))
  })

  it("still refuses unknown fields in a request it sends", () => {
    const request = { input_path: "/a.mkv", output_path: "/a.mp4", workflow: "w", priority: 0, source: "manual", source_reference: null, extra: 1 }

    expect(taskCreateRequestSchema.safeParse(request).success).toBe(false)
  })

  it("parses persisted attempts and drops fields it does not know", () => {
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
    const unknown = taskDetailSchema.parse({ ...detail, logs: ["invented"] })

    // Then: persisted evidence is accepted and the unknown field is dropped.
    expect(parsed.attempts[0]?.remote_input_path).toBe("task/input/../opaque.mkv")
    expect(unknown).toEqual(parsed)
    expect("logs" in unknown).toBe(false)
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
