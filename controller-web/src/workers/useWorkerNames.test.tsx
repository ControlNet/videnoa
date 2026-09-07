import { act, renderHook, waitFor } from "@testing-library/react"
import { describe, expect, it } from "vitest"

import { createApiClient } from "../api/client"
import type { Worker } from "../api/workerSchemas"
import { appWorkerUpdateStore } from "../events/workerUpdates"
import { useWorkerNames } from "./useWorkerNames"

const worker: Worker = {
  id: "550e8400-e29b-41d4-a716-446655440000",
  version: 4,
  name: "render-east",
  api_url: "https://worker.example/api/",
  enabled: true,
  online: true,
  compute_slots: 4,
  capabilities: { workflows: [], refreshed_at: null },
  capacity: {
    used_slots: 0,
    available_slots: 4,
    assigned_tasks: 0,
    staged_tasks: 0,
    processing_tasks: 0,
    active_uploads: 0,
    active_downloads: 0,
    progress: null,
  },
  last_seen_at: null,
  last_assigned_at: null,
  created_at: "2030-01-01T00:00:00Z",
  updated_at: "2030-01-01T00:00:00Z",
  last_error: null,
}

function listApiClient() {
  const requests: Request[] = []
  const fetcher: typeof fetch = async (input, init) => {
    requests.push(new Request(input, init))
    return Response.json({ items: [worker], total: 1 })
  }
  return { apiClient: createApiClient({ fetcher, onUnauthorized: () => undefined }), requests }
}

describe("worker name resolution", () => {
  it("applies a rename from the event stream without another request", async () => {
    // Given: the identifier-to-name map loaded once.
    const { apiClient, requests } = listApiClient()
    const { result } = renderHook(() => useWorkerNames(apiClient))
    await waitFor(() => expect(result.current.get(worker.id)).toBe("render-east"))
    const readsAfterLoad = requests.length

    // When: the worker is renamed elsewhere.
    act(() => appWorkerUpdateStore.publish({ ...worker, version: 5, name: "render-north" }))

    // Then: the map carries the new name and the list was not read again.
    await waitFor(() => expect(result.current.get(worker.id)).toBe("render-north"))
    expect(requests).toHaveLength(readsAfterLoad)
  })

  it("returns the same map when a delta renames nothing", async () => {
    // Given: the loaded map, captured by identity.
    const { apiClient } = listApiClient()
    const { result } = renderHook(() => useWorkerNames(apiClient))
    await waitFor(() => expect(result.current.get(worker.id)).toBe("render-east"))
    const before = result.current

    // When: a delta arrives that changes health but not the name, as most do.
    act(() => appWorkerUpdateStore.publish({ ...worker, version: 6, online: false }))

    // Then: the identical map instance is kept, so the task table is not re-rendered.
    await waitFor(() => expect(result.current).toBe(before))
  })

  it("learns a worker that was registered after the map was read", async () => {
    // Given: the loaded map, which knows one worker.
    const { apiClient } = listApiClient()
    const { result } = renderHook(() => useWorkerNames(apiClient))
    await waitFor(() => expect(result.current.size).toBe(1))

    // When: a worker this map has never seen reports in.
    const added = { ...worker, id: "550e8400-e29b-41d4-a716-4466554400ff", name: "render-west" }
    act(() => appWorkerUpdateStore.publish(added))

    // Then: it is added rather than waiting for the next invalidation.
    await waitFor(() => expect(result.current.get(added.id)).toBe("render-west"))
  })
})
