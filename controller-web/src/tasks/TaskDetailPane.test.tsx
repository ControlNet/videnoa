import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import { describe, expect, it } from "vitest"

import { createApiClient } from "../api/client"
import type { Task, TaskDetail } from "../api/taskSchemas"
import { TaskDetailPane } from "./TaskDetailPane"

type PendingRequest = {
  readonly url: URL
  readonly method: string
  readonly respond: (body: unknown) => void
}

const taskAId = "00000000-0000-4000-8000-000000000031"
const taskBId = "00000000-0000-4000-8000-000000000032"

describe("task detail pane selection", () => {
  it("does not carry an open cancel confirmation over to another task", async () => {
    // Given: the operator opened the cancel confirmation for task A.
    const { apiClient, requests } = controlledApiClient()
    const props = { apiClient, onClose: () => undefined, onChanged: () => undefined }
    const { rerender } = render(<TaskDetailPane {...props} taskId={taskAId} />)
    await respondTo(requests, taskAId, detail(task(taskAId, "/media/task-a.mkv")))
    fireEvent.click(await screen.findByRole("button", { name: "Cancel Task" }))
    expect(screen.getByRole("alertdialog", { name: "Confirm Task Cancellation" })).toBeInTheDocument()

    // When: task B is selected in the table while that confirmation is open.
    rerender(<TaskDetailPane {...props} taskId={taskBId} />)
    await respondTo(requests, taskBId, detail(task(taskBId, "/media/task-b.mkv")))
    await screen.findByRole("heading", { name: /task-b\.mkv/ })

    // Then: task B shows no confirmation it was never given, and nothing was cancelled.
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument()
    expect(requests.filter((request) => request.method === "POST")).toHaveLength(0)
  })
})

function controlledApiClient() {
  const requests: PendingRequest[] = []
  const fetcher: typeof fetch = (input, init) => {
    const request = new Request(input, init)
    return new Promise<Response>((resolve) => {
      requests.push({
        url: new URL(request.url),
        method: request.method,
        respond: (body) => resolve(Response.json(body)),
      })
    })
  }
  return {
    apiClient: createApiClient({ fetcher, onUnauthorized: () => undefined }),
    requests,
  }
}

async function respondTo(requests: readonly PendingRequest[], taskId: string, body: TaskDetail): Promise<void> {
  const isDetail = (request: PendingRequest) => request.method === "GET" && request.url.pathname === `/api/tasks/${taskId}`
  await waitFor(() => expect(requests.some(isDetail)).toBe(true))
  requests.filter(isDetail).at(-1)?.respond(body)
}

function detail(taskValue: Task): TaskDetail {
  return { task: taskValue, attempts: [], total: 0, limit: 100, offset: 0 }
}

function task(id: string, inputPath: string): Task {
  return {
    id,
    version: 1,
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
    worker_id: "00000000-0000-4000-8000-000000000033",
    requested_worker_id: null,
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
  }
}
