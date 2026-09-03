import { describe, expect, it } from "vitest"

import type { Task, TaskStatusCounts } from "../api/taskSchemas"
import { canMergeTaskUpdate, counterValues } from "./model"
import { parseTaskQuery } from "./query"

const task = {
  id: "550e8400-e29b-41d4-a716-446655440000",
  version: 3,
  status: "processing",
  input_path: "/media/episode-01.mkv",
  output_path: "/output/episode-01.mp4",
  input_extension: "mkv",
  output_extension: "mp4",
  workflow: "anime-2x",
  priority: 1,
  source: "manual",
  source_reference: null,
  input_size: 1024,
  worker_id: "550e8400-e29b-41d4-a716-446655440001",
  remote_job_id: null,
  progress: {
    percent: 30,
    processed_frames: 300,
    total_frames: 1000,
    frames_per_second: 24,
    eta_seconds: 30,
    bytes_transferred: null,
    bytes_total: null,
  },
  attempt_count: 1,
  failure: null,
  cancel_requested_at: null,
  created_at: "2030-01-01T00:00:00Z",
  updated_at: "2030-01-01T00:01:00Z",
  completed_at: null,
} as const satisfies Task

describe("task live model", () => {
  it("merges only newer matching active progress updates", () => {
    // Given: an active row in a path-filtered page.
    const query = parseTaskQuery(new URLSearchParams("status=processing&search=episode"))

    // When: a newer update changes only progress-bearing fields.
    const incoming = { ...task, version: 4, progress: { ...task.progress, percent: 42 } }

    // Then: the row can be replaced without refetching page membership or order.
    expect(canMergeTaskUpdate(task, incoming, query)).toBe(true)
    expect(canMergeTaskUpdate(incoming, task, query)).toBe(false)
    expect(canMergeTaskUpdate(task, { ...incoming, status: "completed", completed_at: "2030-01-01T00:02:00Z" }, query)).toBe(false)
  })

  it("rejects updates that change the active ordering field", () => {
    // Given: a priority-sorted active task page.
    const query = parseTaskQuery(new URLSearchParams("sort=priority"))

    // When: a newer update changes the row's priority.
    const incoming = { ...task, version: 4, priority: task.priority + 1 }

    // Then: the row requires a bounded page refetch instead of an in-place merge.
    expect(canMergeTaskUpdate(task, incoming, query)).toBe(false)
  })

  it("derives compact counters from all fourteen server statuses", () => {
    // Given: API truth containing every durable task status.
    const counts = {
      items: [
        ["queued", 2], ["reserved", 1], ["uploading", 1], ["staged", 1],
        ["submitting", 1], ["processing", 3], ["remote_completed", 1],
        ["downloading", 1], ["verifying", 1], ["publishing", 1],
        ["remote_cleanup", 1], ["completed", 5], ["failed", 2], ["cancelled", 1],
      ].map(([status, count]) => ({ status, count })),
      total: 22,
    } as TaskStatusCounts

    // When: the task surface groups operational counts.
    const values = counterValues(counts)

    // Then: active, processing, failure, and finished totals preserve API truth.
    expect(values).toEqual({ all: 22, active: 14, queued: 2, processing: 11, failed: 2, finished: 6 })
  })
})
