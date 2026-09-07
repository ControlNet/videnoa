import { act, renderHook, waitFor } from "@testing-library/react"
import { describe, expect, it } from "vitest"

import { createApiClient } from "../api/client"
import type { Task, TaskAttempt, TaskDetail } from "../api/taskSchemas"
import { appTaskUpdateStore } from "../events/taskUpdates"
import { useTaskDetail } from "./useTaskDetail"

type PendingRequest = {
  readonly url: URL
  readonly signal: AbortSignal
  readonly respond: (body: unknown) => void
}

const taskAId = "00000000-0000-4000-8000-000000000021"
const taskBId = "00000000-0000-4000-8000-000000000022"

describe("task detail history ownership", () => {
  it("does not let delayed task A history corrupt newly selected task B", async () => {
    // Given: task A is loaded and its next history page remains pending.
    const { apiClient, requests } = controlledApiClient()
    const taskA = task(taskAId, "/media/task-a.mkv", 1)
    const taskB = task(taskBId, "/media/task-b.mkv", 1)
    const taskANewest = attempt(taskA, 2)
    const taskAOlder = attempt(taskA, 1)
    const taskBOnly = attempt(taskB, 1)
    const { result, rerender } = renderHook(
      ({ taskId }) => useTaskDetail(apiClient, taskId),
      { initialProps: { taskId: taskAId } },
    )
    await respondTo(requests, taskAId, 0, detail(taskA, [taskANewest], 2))
    await waitFor(() => expect(result.current.detail?.task.id).toBe(taskAId))
    act(() => result.current.loadMore())
    await waitFor(() => expect(requestsFor(requests, taskAId, 1)).toHaveLength(1))

    // When: task B becomes authoritative before task A's delayed page resolves.
    rerender({ taskId: taskBId })
    await respondTo(requests, taskBId, 0, detail(taskB, [taskBOnly], 1))
    await waitFor(() => expect(result.current.detail?.task.id).toBe(taskBId))
    await act(async () => {
      findRequest(requests, taskAId, 1).respond(detail(taskA, [taskAOlder], 2))
      await Promise.resolve()
    })

    // Then: task B remains selected with only task B attempts.
    await waitFor(() => expect(result.current.loadingMore).toBe(false))
    expect(result.current.detail?.task.id).toBe(taskBId)
    expect(result.current.detail?.attempts.map(({ id }) => id)).toEqual([taskBOnly.id])
  })

  it("does not append an invalidated history page after a newer SSE reload", async () => {
    // Given: one task detail is loaded and its next history page remains pending.
    const { apiClient, requests } = controlledApiClient()
    const originalTask = task(taskAId, "/media/task-a.mkv", 1)
    const updatedTask = task(taskAId, "/media/task-a.mkv", 2)
    const newest = attempt(updatedTask, 2)
    const older = attempt(updatedTask, 1)
    const { result } = renderHook(() => useTaskDetail(apiClient, taskAId))
    await respondTo(requests, taskAId, 0, detail(originalTask, [newest], 2))
    await waitFor(() => expect(result.current.detail?.task.version).toBe(1))
    act(() => result.current.loadMore())
    await waitFor(() => expect(requestsFor(requests, taskAId, 1)).toHaveLength(1))

    // When: SSE invalidates detail, the replacement page wins, then the stale page resolves.
    act(() => appTaskUpdateStore.publish(updatedTask))
    await waitFor(() => expect(requestsFor(requests, taskAId, 0)).toHaveLength(2))
    requestsFor(requests, taskAId, 0)[1]?.respond(detail(updatedTask, [newest, older], 2))
    await waitFor(() => expect(result.current.detail?.task.version).toBe(2))
    await act(async () => {
      findRequest(requests, taskAId, 1).respond(detail(originalTask, [older], 2))
      await Promise.resolve()
    })

    // Then: the newer generation keeps unique newest-to-oldest attempts and coherent totals.
    await waitFor(() => expect(result.current.loadingMore).toBe(false))
    expect(result.current.detail?.task.version).toBe(2)
    expect(result.current.detail?.attempts.map(({ id }) => id)).toEqual([newest.id, older.id])
    expect(result.current.detail?.attempts.length).toBeLessThanOrEqual(result.current.detail?.total ?? 0)
  })
})

describe("task detail refresh window", () => {
  it("keeps attempts beyond the page maximum when a refresh cannot re-read them", async () => {
    // Given: a task with more persisted history than one page can carry, expanded
    // to 600 attempts through five explicit next-page actions.
    const { apiClient, requests } = controlledApiClient()
    const originalTask = task(taskAId, "/media/task-a.mkv", 1)
    const updatedTask = task(taskAId, "/media/task-a.mkv", 2)
    const total = 700
    const history = Array.from({ length: total }, (_, index) => attempt(originalTask, total - index))
    const { result } = renderHook(() => useTaskDetail(apiClient, taskAId))
    await respondTo(requests, taskAId, 0, detail(originalTask, history.slice(0, 100), total))
    await waitFor(() => expect(result.current.detail?.attempts).toHaveLength(100))
    for (let offset = 100; offset < 600; offset += 100) {
      act(() => result.current.loadMore())
      await respondTo(requests, taskAId, offset, detail(originalTask, history.slice(offset, offset + 100), total, offset))
      await waitFor(() => expect(result.current.detail?.attempts).toHaveLength(offset + 100))
    }

    // When: SSE invalidates detail, so the refresh asks for the open window and is
    // clamped to what one request may return.
    act(() => appTaskUpdateStore.publish(updatedTask))
    await waitFor(() => expect(requestsFor(requests, taskAId, 0)).toHaveLength(2))
    const refresh = findRequest(requests, taskAId, 0)
    expect(refresh.url.searchParams.get("limit")).toBe("500")
    await act(async () => {
      refresh.respond(detail(updatedTask, history.slice(0, 500), total))
      await Promise.resolve()
    })

    // Then: the refreshed head is authoritative and the attempts it could not
    // reach are carried, so the operator never sees fewer than they expanded to.
    await waitFor(() => expect(result.current.detail?.task.version).toBe(2))
    expect(result.current.detail?.attempts.map(({ id }) => id)).toEqual(history.slice(0, 600).map(({ id }) => id))
  })

  it("re-reads only the default page when nothing has been expanded", async () => {
    // Given: a freshly opened task detail showing its first page.
    const { apiClient, requests } = controlledApiClient()
    const originalTask = task(taskAId, "/media/task-a.mkv", 1)
    const updatedTask = task(taskAId, "/media/task-a.mkv", 2)
    const newest = attempt(originalTask, 2)
    const { result } = renderHook(() => useTaskDetail(apiClient, taskAId))
    await respondTo(requests, taskAId, 0, detail(originalTask, [newest], 2))
    await waitFor(() => expect(result.current.detail?.task.version).toBe(1))

    // When: SSE invalidates detail.
    act(() => appTaskUpdateStore.publish(updatedTask))

    // Then: the refresh stays at the default page rather than growing the request.
    await waitFor(() => expect(requestsFor(requests, taskAId, 0)).toHaveLength(2))
    expect(findRequest(requests, taskAId, 0).url.searchParams.get("limit")).toBe("100")
  })
})

function controlledApiClient() {
  const requests: PendingRequest[] = []
  const fetcher: typeof fetch = (input, init) => {
    const request = new Request(input, init)
    return new Promise<Response>((resolve) => {
      requests.push({
        url: new URL(request.url),
        signal: request.signal,
        respond: (body) => resolve(Response.json(body)),
      })
    })
  }
  return {
    apiClient: createApiClient({ fetcher, onUnauthorized: () => undefined }),
    requests,
  }
}

async function respondTo(requests: readonly PendingRequest[], taskId: string, offset: number, body: TaskDetail): Promise<void> {
  await waitFor(() => expect(requestsFor(requests, taskId, offset).length).toBeGreaterThan(0))
  findRequest(requests, taskId, offset).respond(body)
}

function findRequest(requests: readonly PendingRequest[], taskId: string, offset: number): PendingRequest {
  const request = requestsFor(requests, taskId, offset).at(-1)
  if (request === undefined) throw new RangeError(`missing detail request for ${taskId} at ${offset}`)
  return request
}

function requestsFor(requests: readonly PendingRequest[], taskId: string, offset: number): readonly PendingRequest[] {
  return requests.filter((request) => request.url.pathname === `/api/tasks/${taskId}` && request.url.searchParams.get("offset") === String(offset))
}

/* `offset` defaults to 0 so a page answered at the wrong offset is rejected, which is what the ownership tests assert. */
function detail(taskValue: Task, attempts: readonly TaskAttempt[], total: number, offset = 0): TaskDetail {
  return { task: taskValue, attempts: [...attempts], total, limit: 100, offset }
}

function task(id: string, inputPath: string, version: number): Task {
  return {
    id,
    version,
    status: "processing",
    input_path: inputPath,
    output_path: inputPath.replace("/media/", "/output/").replace(".mkv", ".mp4"),
    input_extension: "mkv",
    output_extension: "mp4",
    workflow: "anime-2x",
    priority: 1,
    source: "manual",
    source_reference: null,
    input_size: 1024,
    worker_id: "00000000-0000-4000-8000-000000000023",
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
    attempt_count: 2,
    failure: null,
    cancel_requested_at: null,
    created_at: "2030-01-01T00:00:00Z",
    updated_at: "2030-01-01T00:01:00Z",
    completed_at: null,
  }
}

function attempt(taskValue: Task, attemptNumber: number): TaskAttempt {
  const suffix = String(attemptNumber).padStart(12, "0")
  return {
    id: `10000000-0000-4000-8000-${suffix}`,
    task_id: taskValue.id,
    attempt_number: attemptNumber,
    worker_id: taskValue.worker_id,
    status: taskValue.status,
    submission_key: `20000000-0000-4000-8000-${suffix}`,
    remote_job_id: null,
    remote_input_path: null,
    remote_output_path: null,
    progress: taskValue.progress,
    retry: { retry_count: 0, next_retry_at: null },
    failure: null,
    created_at: taskValue.created_at,
    started_at: taskValue.created_at,
    completed_at: null,
  }
}
