import { act, renderHook, waitFor } from "@testing-library/react"
import { describe, expect, it, vi } from "vitest"

import { createApiClient } from "../api/client"
import type { Task } from "../api/taskSchemas"
import type { Worker } from "../api/workerSchemas"
import { appInvalidationStore } from "../events/store"
import { appTaskUpdateStore } from "../events/taskUpdates"
import { appWorkerUpdateStore } from "../events/workerUpdates"
import { useWorkersData } from "./useWorkersData"

const worker: Worker = {
  id: "550e8400-e29b-41d4-a716-446655440000",
  version: 4,
  name: "render-east",
  api_url: "https://worker.example/api/",
  enabled: true,
  online: true,
  compute_slots: 4,
  capabilities: { workflows: [{ name: "anime-2x", kind: "workflow" }], invalid_workflows: [], refreshed_at: "2030-01-01T00:00:00Z" },
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

function listApiClient() {
  const requests: Request[] = []
  const fetcher: typeof fetch = async (input, init) => {
    const request = new Request(input, init)
    requests.push(request)
    return Response.json({ items: [worker], total: 1 })
  }
  return { apiClient: createApiClient({ fetcher, onUnauthorized: () => undefined }), requests }
}

describe("worker deltas", () => {
  it("preserves two worker deltas queued before either state update runs", async () => {
    // Synthetic test-only workers expose stale-list overwrites across microtasks.
    const other = { ...worker, id: "550e8400-e29b-41d4-a716-446655440001", name: "render-west" }
    const apiClient = createApiClient({
      fetcher: async () => Response.json({ items: [worker, other], total: 2 }),
      onUnauthorized: () => undefined,
    })
    const { result } = renderHook(() => useWorkersData(apiClient))
    await waitFor(() => expect(result.current.workers?.items).toHaveLength(2))
    const pending: VoidFunction[] = []
    const microtasks = vi.spyOn(globalThis, "queueMicrotask").mockImplementation((callback) => pending.push(callback))
    try {
      act(() => appWorkerUpdateStore.publish({ ...worker, version: 5, online: false }))
      act(() => appWorkerUpdateStore.publish({ ...other, version: 5, online: false }))
      expect(pending).toHaveLength(2)
      act(() => pending.forEach((callback) => callback()))
      expect(result.current.workers?.items.map((item) => item.online)).toEqual([false, false])
    } finally {
      microtasks.mockRestore()
    }
  })

  it("replaces a listed worker in place without another request", async () => {
    // Given: one loaded worker reported online.
    const { apiClient, requests } = listApiClient()
    const { result } = renderHook(() => useWorkersData(apiClient))
    await waitFor(() => expect(result.current.workers?.items).toHaveLength(1))
    const readsAfterLoad = requests.filter((request) => request.method === "GET").length

    // When: the event stream reports it offline at a newer version.
    act(() => appWorkerUpdateStore.publish({ ...worker, version: 5, online: false, last_error: "probe timed out" }))

    // Then: the row carries the new health and the list was not refetched.
    await waitFor(() => expect(result.current.workers?.items[0]?.online).toBe(false))
    expect(result.current.workers?.items[0]?.last_error).toBe("probe timed out")
    expect(requests.filter((request) => request.method === "GET")).toHaveLength(readsAfterLoad)
  })

  it("ignores a delta that is not newer than the listed version", async () => {
    // Given: one loaded worker at version 4.
    const { apiClient, requests } = listApiClient()
    const { result } = renderHook(() => useWorkersData(apiClient))
    await waitFor(() => expect(result.current.workers?.items).toHaveLength(1))
    const readsAfterLoad = requests.filter((request) => request.method === "GET").length

    // When: a delta arrives at the same version with contradictory health, as a
    // late event can after the list has already been read.
    act(() => appWorkerUpdateStore.publish({ ...worker, version: 4, online: false }))

    // Then: the listed representation stands and nothing is refetched.
    await waitFor(() => expect(requests.filter((request) => request.method === "GET")).toHaveLength(readsAfterLoad))
    expect(result.current.workers?.items[0]?.online).toBe(true)
  })

  it("refetches once for a worker the route has never listed", async () => {
    // Given: one loaded worker.
    const { apiClient, requests } = listApiClient()
    const { result } = renderHook(() => useWorkersData(apiClient))
    await waitFor(() => expect(result.current.workers?.items).toHaveLength(1))
    const readsAfterLoad = requests.filter((request) => request.method === "GET").length

    // When: a delta names a worker that is not in the list, so its position and
    // the total are not the client's to decide.
    act(() => appWorkerUpdateStore.publish({ ...worker, id: "550e8400-e29b-41d4-a716-446655440077", name: "render-west" }))

    // Then: exactly one bounded list read settles it.
    await waitFor(() => expect(requests.filter((request) => request.method === "GET")).toHaveLength(readsAfterLoad + 1))
  })
})

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

  it("coalesces task updates while GET /api/workers is in flight without aborting it", async () => {
    const pending: Array<{ readonly signal: AbortSignal; readonly resolve: (body: unknown) => void }> = []
    const fetcher: typeof fetch = (_input, init) => {
      const signal = init?.signal ?? new AbortController().signal
      return new Promise<Response>((resolve) => {
        pending.push({ signal, resolve: (body) => resolve(Response.json(body)) })
      })
    }
    const apiClient = createApiClient({ fetcher, onUnauthorized: () => undefined })
    const { result } = renderHook(() => useWorkersData(apiClient))
    await waitFor(() => expect(pending).toHaveLength(1))

    // Burst of task progress events while initial GET /api/workers is still pending.
    const sampleTask: Task = {
      id: "00000000-0000-4000-8000-000000000091",
      version: 1,
      status: "processing",
      input_path: "/media/a.mkv",
      output_path: "/output/a.mp4",
      input_extension: "mkv",
      output_extension: "mp4",
      workflow: "anime-2x",
      priority: 0,
      source: "manual",
      source_reference: null,
      input_size: 1024,
      worker_id: worker.id,
      requested_worker_id: null,
      remote_job_id: null,
      progress: {
        percent: 25,
        processed_frames: 25,
        total_frames: 100,
        frames_per_second: 12,
        eta_seconds: 6,
        bytes_transferred: null,
        bytes_total: null,
      },
      attempt_count: 1,
      failure: null,
      cancel_requested_at: null,
      created_at: "2030-01-01T00:00:00Z",
      updated_at: "2030-01-01T00:01:00Z",
      completed_at: null,
      allowed_actions: [],
    }
    act(() => {
      appTaskUpdateStore.publish({ ...sampleTask, version: 2 })
      appTaskUpdateStore.publish({ ...sampleTask, version: 3 })
      appTaskUpdateStore.publish({ ...sampleTask, version: 4 })
    })

    // First GET must not be aborted or starved by task events.
    expect(pending).toHaveLength(1)
    expect(pending[0]?.signal.aborted).toBe(false)

    // Resolving the first GET renders the snapshot and triggers one trailing refresh.
    await act(async () => {
      pending[0]?.resolve({ items: [worker], total: 1 })
      await Promise.resolve()
    })
    await waitFor(() => expect(result.current.workers?.items).toHaveLength(1))
    await waitFor(() => expect(pending).toHaveLength(2))
    expect(pending[1]?.signal.aborted).toBe(false)

    const updatedWorker: Worker = {
      ...worker,
      capacity: { ...worker.capacity, used_slots: 3, available_slots: 1 },
    }
    await act(async () => {
      pending[1]?.resolve({ items: [updatedWorker], total: 1 })
      await Promise.resolve()
    })
    await waitFor(() => expect(result.current.workers?.items[0]?.capacity.used_slots).toBe(3))
    expect(pending).toHaveLength(2)
  })

  it("replays worker deltas received while GET /api/workers is in flight", async () => {
    const pending: Array<{ readonly resolve: (body: unknown) => void }> = []
    const fetcher: typeof fetch = () => new Promise<Response>((resolve) => {
      pending.push({ resolve: (body) => resolve(Response.json(body)) })
    })
    const apiClient = createApiClient({ fetcher, onUnauthorized: () => undefined })
    const { result } = renderHook(() => useWorkersData(apiClient))
    await waitFor(() => expect(pending).toHaveLength(1))

    // A newer worker delta arrives while the GET snapshot (version 4) is still in flight.
    act(() => appWorkerUpdateStore.publish({ ...worker, version: 5, online: false, last_error: "lost connection" }))
    await act(async () => {
      pending[0]?.resolve({ items: [worker], total: 1 })
      await Promise.resolve()
    })

    await waitFor(() => expect(result.current.workers?.items[0]?.version).toBe(5))
    expect(result.current.workers?.items[0]?.online).toBe(false)
    expect(result.current.workers?.items[0]?.last_error).toBe("lost connection")
    expect(pending).toHaveLength(1)
  })
})
