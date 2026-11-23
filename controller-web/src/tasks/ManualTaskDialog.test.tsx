import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { describe, expect, it, vi } from "vitest"

import { createApiClient } from "../api/client"
import type { Task } from "../api/taskSchemas"
import { ManualTaskDialog } from "./ManualTaskDialog"

const createdTask: Task = {
  id: "00000000-0000-4000-8000-000000000044",
  version: 1,
  status: "queued",
  input_path: "/media/in.mkv",
  output_path: "/media/out.mp4",
  input_extension: "mkv",
  output_extension: "mp4",
  workflow: "anime-2x",
  priority: 0,
  source: "manual",
  source_reference: null,
  input_size: 1024,
  worker_id: null,
  requested_worker_id: null,
  remote_job_id: null,
  progress: {
    percent: 0,
    processed_frames: null,
    total_frames: null,
    frames_per_second: null,
    eta_seconds: null,
    bytes_transferred: null,
    bytes_total: null,
  },
  attempt_count: 0,
  failure: null,
  cancel_requested_at: null,
  created_at: "2030-01-01T00:00:00Z",
  updated_at: "2030-01-01T00:00:00Z",
  completed_at: null,
  allowed_actions: ["cancel"],
}

describe("ManualTaskDialog", () => {
  it("prevents duplicate POST /api/tasks requests on rapid double submit", async () => {
    const postRequests: Request[] = []
    let resolvePost: ((response: Response) => void) | null = null
    const fetcher: typeof fetch = (input, init) => {
      const request = new Request(input, init)
      if (request.method === "POST") {
        postRequests.push(request)
        return new Promise<Response>((resolve) => {
          resolvePost = resolve
        })
      }
      return Promise.resolve(Response.json({ items: [] }))
    }
    const apiClient = createApiClient({ fetcher, onUnauthorized: () => undefined })
    const onCreated = vi.fn()
    const onClose = vi.fn()

    render(<ManualTaskDialog apiClient={apiClient} open onClose={onClose} onCreated={onCreated} />)

    fireEvent.change(screen.getByLabelText("Input Path"), { target: { value: "/media/in.mkv" } })
    fireEvent.change(screen.getByLabelText("Output Path"), { target: { value: "/media/out.mp4" } })
    fireEvent.change(screen.getByLabelText("Workflow"), { target: { value: "anime-2x" } })

    const form = screen.getByRole("button", { name: "Create Task" }).closest("form")
    if (form === null) throw new Error("expected task dialog form")

    // Submit twice synchronously before React disables the submit button.
    act(() => {
      fireEvent.submit(form)
      fireEvent.submit(form)
    })

    await waitFor(() => expect(postRequests).toHaveLength(1))
    expect(postRequests[0]?.headers.get("Idempotency-Key")).toBeTruthy()

    await act(async () => {
      resolvePost?.(Response.json(createdTask, { status: 201 }))
      await Promise.resolve()
    })

    await waitFor(() => expect(onCreated).toHaveBeenCalledTimes(1))
    expect(postRequests).toHaveLength(1)
    expect(onClose).toHaveBeenCalledTimes(1)
  })
})
