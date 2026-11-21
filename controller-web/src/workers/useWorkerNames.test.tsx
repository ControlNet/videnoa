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
  capabilities: { workflows: [], invalid_workflows: [], refreshed_at: null },
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

function listApiClient(items: readonly Worker[] = [worker]) {
  const requests: Request[] = []
  const fetcher: typeof fetch = async (input, init) => {
    requests.push(new Request(input, init))
    return Response.json({ items, total: items.length })
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

  it("keeps the final name and map identity when a batched rename returns to the original", async () => {
    // Given: the loaded map with worker named render-east, captured by identity.
    const { apiClient, requests } = listApiClient()
    const { result, unmount } = renderHook(() => useWorkerNames(apiClient))
    await waitFor(() => expect(result.current.get(worker.id)).toBe("render-east"))
    const before = result.current

    // When: a single batch renames the worker away and back to its original name.
    await act(async () => {
      appWorkerUpdateStore.publish({ ...worker, version: 5, name: "render-north" })
      appWorkerUpdateStore.publish({ ...worker, version: 6, name: "render-east" })
    })

    // Then: the final original name and map instance are preserved with no extra fetch.
    expect(requests).toHaveLength(1)
    expect(result.current.get(worker.id)).toBe("render-east")
    expect(result.current).toBe(before)
    unmount()
  })

  it("applies the final name when a batched rename settles on a different name", async () => {
    // Given: the loaded map with worker named render-east.
    const { apiClient, requests } = listApiClient()
    const { result, unmount } = renderHook(() => useWorkerNames(apiClient))
    await waitFor(() => expect(result.current.get(worker.id)).toBe("render-east"))
    const before = result.current

    // When: a single batch renames the worker twice to a distinct final name.
    await act(async () => {
      appWorkerUpdateStore.publish({ ...worker, version: 5, name: "render-north" })
      appWorkerUpdateStore.publish({ ...worker, version: 6, name: "render-south" })
    })

    // Then: the map updates to the latest name in the batch without refetching.
    expect(requests).toHaveLength(1)
    expect(result.current.get(worker.id)).toBe("render-south")
    expect(result.current).not.toBe(before)
    unmount()
  })

  it("applies multi-worker batched updates across round-trip, renamed, and added workers", async () => {
    // Given: two initially listed workers.
    const secondWorker: Worker = {
      ...worker,
      id: "550e8400-e29b-41d4-a716-446655440001",
      name: "render-west",
    }
    const addedWorker: Worker = {
      ...worker,
      id: "550e8400-e29b-41d4-a716-446655440002",
      name: "render-staging",
    }
    const { apiClient, requests } = listApiClient([worker, secondWorker])
    const { result, unmount } = renderHook(() => useWorkerNames(apiClient))
    await waitFor(() => expect(result.current.size).toBe(2))
    const before = result.current

    // When: one batch round-trips the first worker, renames the second, and registers a third twice.
    await act(async () => {
      appWorkerUpdateStore.publish({ ...worker, version: 5, name: "render-north" })
      appWorkerUpdateStore.publish({ ...secondWorker, version: 5, name: "render-central" })
      appWorkerUpdateStore.publish({ ...addedWorker, version: 1, name: "render-staging" })
      appWorkerUpdateStore.publish({ ...worker, version: 6, name: "render-east" })
      appWorkerUpdateStore.publish({ ...addedWorker, version: 2, name: "render-south" })
    })

    // Then: each worker resolves to its final name in the batch without another fetch.
    expect(requests).toHaveLength(1)
    expect(result.current).not.toBe(before)
    expect(result.current.get(worker.id)).toBe("render-east")
    expect(result.current.get(secondWorker.id)).toBe("render-central")
    expect(result.current.get(addedWorker.id)).toBe("render-south")
    unmount()
  })

  it("preserves map identity when a multi-worker batch produces no net name change", async () => {
    // Given: two initially listed workers, captured by map identity.
    const secondWorker: Worker = {
      ...worker,
      id: "550e8400-e29b-41d4-a716-446655440003",
      name: "render-west",
    }
    const { apiClient, requests } = listApiClient([worker, secondWorker])
    const { result, unmount } = renderHook(() => useWorkerNames(apiClient))
    await waitFor(() => expect(result.current.size).toBe(2))
    const before = result.current

    // When: a batch round-trips one worker's name and updates the other's health only.
    await act(async () => {
      appWorkerUpdateStore.publish({ ...worker, version: 5, name: "render-north" })
      appWorkerUpdateStore.publish({ ...secondWorker, version: 5, online: false })
      appWorkerUpdateStore.publish({ ...worker, version: 6, name: "render-east" })
    })

    // Then: the identical map instance is retained and both names stay intact.
    expect(requests).toHaveLength(1)
    expect(result.current).toBe(before)
    expect(result.current.get(worker.id)).toBe("render-east")
    expect(result.current.get(secondWorker.id)).toBe("render-west")
    unmount()
  })

  it("preserves a newer SSE rename and newly registered worker when an older GET finishes later", async () => {
    // Given: an in-flight GET /api/workers request that captured version 4 ("render-east").
    let resolveList!: (response: Response) => void
    const requests: Request[] = []
    const fetcher: typeof fetch = async (input, init) => {
      requests.push(new Request(input, init))
      return new Promise((resolve) => {
        resolveList = resolve
      })
    }
    const apiClient = createApiClient({ fetcher, onUnauthorized: () => undefined })
    const { result, unmount } = renderHook(() => useWorkerNames(apiClient))
    await waitFor(() => expect(requests).toHaveLength(1))

    // When: an SSE rename (version 5) and a newly registered worker arrive before GET resolves.
    const addedWorker: Worker = {
      ...worker,
      id: "550e8400-e29b-41d4-a716-4466554400aa",
      version: 1,
      name: "render-new",
    }
    act(() => {
      appWorkerUpdateStore.publish({ ...worker, version: 5, name: "render-north" })
      appWorkerUpdateStore.publish(addedWorker)
    })
    await waitFor(() => expect(result.current.get(worker.id)).toBe("render-north"))
    expect(result.current.get(addedWorker.id)).toBe("render-new")
    const afterSse = result.current

    // And the stale GET response (version 4) arrives afterwards.
    await act(async () => {
      resolveList(Response.json({ items: [worker], total: 1 }))
    })

    // Then: neither the newer rename nor the newly registered worker is clobbered, and map identity holds.
    expect(result.current.get(worker.id)).toBe("render-north")
    expect(result.current.get(addedWorker.id)).toBe("render-new")
    expect(result.current).toBe(afterSse)
    unmount()
  })
})
