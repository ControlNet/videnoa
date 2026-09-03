import { act, renderHook, waitFor } from "@testing-library/react"
import { describe, expect, it } from "vitest"

import { createApiClient } from "../api/client"
import type { Worker } from "../api/workerSchemas"
import { appInvalidationStore } from "../events/store"
import { useWorkersData } from "./useWorkersData"

const worker: Worker = {
  id: "550e8400-e29b-41d4-a716-446655440000",
  version: 4,
  name: "render-east",
  api_url: "https://worker.example/api/",
  enabled: true,
  online: true,
  compute_slots: 4,
  capabilities: { workflows: [{ name: "anime-2x", kind: "workflow" }], refreshed_at: "2030-01-01T00:00:00Z" },
  capacity: {
    used_slots: 2,
    available_slots: 2,
    assigned_tasks: 3,
    staged_tasks: 1,
    processing_tasks: 2,
    active_uploads: 1,
    active_downloads: 0,
    progress: null,
  },
  last_seen_at: "2030-01-01T00:01:00Z",
  last_assigned_at: "2030-01-01T00:00:30Z",
  created_at: "2030-01-01T00:00:00Z",
  updated_at: "2030-01-01T00:01:00Z",
  last_error: null,
}

describe("worker data requests", () => {
  it("refetches authoritative workers after a stale mutation", async () => {
    // Given: one loaded worker whose disable request conflicts.
    const requests: Request[] = []
    const fetcher: typeof fetch = async (input, init) => {
      const request = new Request(input, init)
      requests.push(request)
      if (request.method === "POST") {
        return Response.json({ error: { code: "conflict", message: "worker changed since it was read", retryable: false, field_errors: [] } }, { status: 409 })
      }
      return Response.json({ items: [worker], total: 1 })
    }
    const apiClient = createApiClient({ fetcher, onUnauthorized: () => undefined })
    const { result } = renderHook(() => useWorkersData(apiClient))
    await waitFor(() => expect(result.current.workers?.items).toHaveLength(1))

    // When: the operator disables the stale row.
    await act(async () => result.current.setEnabled(worker, false))

    // Then: the version is submitted and the authoritative list is fetched once more.
    await waitFor(() => expect(requests.filter((request) => request.method === "GET")).toHaveLength(2))
    expect(await requests.find((request) => request.method === "POST")?.json()).toEqual({ version: 4 })
    expect(result.current.actionError?.code).toBe("conflict")
  })

  it("baselines retained invalidation generation at mount", async () => {
    // Given: an invalidation retained before the Workers route mounts.
    appInvalidationStore.invalidate("reconnect")
    let reads = 0
    const apiClient = createApiClient({
      fetcher: async () => {
        reads += 1
        return Response.json({ items: [], total: 0 })
      },
      onUnauthorized: () => undefined,
    })

    // When: the hook mounts and then receives one new authoritative invalidation.
    renderHook(() => useWorkersData(apiClient))
    await waitFor(() => expect(reads).toBe(1))
    act(() => appInvalidationStore.invalidate("lag"))

    // Then: retained history causes no duplicate fetch, while the new generation does.
    await waitFor(() => expect(reads).toBe(2))
  })
})
