import { renderHook, waitFor } from "@testing-library/react"
import { describe, expect, it } from "vitest"

import { createApiClient } from "../api/client"
import { parseTaskQuery } from "./query"
import { useTasksData } from "./useTasksData"

const initialTask = {
  id: "550e8400-e29b-41d4-a716-446655440000",
  version: 1,
  status: "processing",
  input_path: "/media/old-query.mkv",
  output_path: "/output/old-query.mp4",
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
} as const

const emptyCounts = {
  items: [
    "queued", "reserved", "uploading", "staged", "submitting", "processing", "remote_completed",
    "downloading", "verifying", "publishing", "remote_cleanup", "completed", "failed", "cancelled",
  ].map((status) => ({ status, count: 0 })),
  total: 0,
}

describe("task data requests", () => {
  it("does not retain rows when the current query fails", async () => {
    // Given: a loaded page followed by a different URL query that fails.
    const fetcher: typeof fetch = async (input, init) => {
      const request = new Request(input, init)
      const url = new URL(request.url)
      if (url.pathname === "/api/status-counts") {
        return Response.json(emptyCounts)
      }
      if (url.searchParams.get("search") === "new-query") {
        return Response.json({ error: { code: "unavailable", message: "busy", retryable: true, field_errors: [] } }, { status: 503 })
      }
      return Response.json({ items: [initialTask], total: 1, limit: 50, offset: 0 })
    }
    const apiClient = createApiClient({ fetcher, onUnauthorized: () => undefined })
    const initialQuery = parseTaskQuery(new URLSearchParams())
    const { result, rerender } = renderHook(
      ({ query }) => useTasksData(apiClient, query),
      { initialProps: { query: initialQuery } },
    )
    await waitFor(() => expect(result.current.page?.items[0]?.input_path).toBe("/media/old-query.mkv"))

    // When: the request generation represented by the new query fails.
    rerender({ query: parseTaskQuery(new URLSearchParams("search=new-query")) })
    await waitFor(() => expect(result.current.error).toBe("Controller could not load task history."))

    // Then: the previous query's rows cannot render under the new URL.
    expect(result.current.page).toBeNull()
  })
})
