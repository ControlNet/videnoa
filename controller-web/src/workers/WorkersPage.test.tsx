import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import { describe, expect, it } from "vitest"

import { createApiClient } from "../api/client"
import { WorkersPage } from "./WorkersPage"

describe("Workers page", () => {
  it("renders dense operational state and an accessible add dialog", async () => {
    // Given: one offline enabled worker with durable activity and an error.
    const apiClient = createApiClient({
      fetcher: async () => Response.json({
        items: [{
          id: "550e8400-e29b-41d4-a716-446655440000",
          version: 4,
          name: "render-east",
          api_url: "https://worker.example/api/",
          enabled: true,
          online: false,
          compute_slots: 4,
          capabilities: { workflows: [], refreshed_at: null },
          capacity: { used_slots: 2, available_slots: 2, assigned_tasks: 3, staged_tasks: 1, processing_tasks: 2, active_uploads: 1, active_downloads: 1, progress: null },
          last_seen_at: null,
          last_assigned_at: null,
          created_at: "2030-01-01T00:00:00Z",
          updated_at: "2030-01-01T00:01:00Z",
          last_error: "health check failed",
        }],
        total: 1,
      }),
      onUnauthorized: () => undefined,
    })
    render(<WorkersPage apiClient={apiClient} />)

    // When: the worker table loads and Add Worker is opened.
    expect(await screen.findByText("render-east")).toBeInTheDocument()
    expect(screen.getByRole("region", { name: "Scrollable worker results" })).toHaveAttribute("tabindex", "0")
    fireEvent.click(screen.getByRole("button", { name: "Add Worker" }))

    // Then: health, policy, capacity, transfer state, and labelled inputs are exposed.
    expect(screen.getByText("Offline")).toBeInTheDocument()
    expect(screen.getByText("Enabled")).toBeInTheDocument()
    expect(screen.getByText("2 / 4")).toBeInTheDocument()
    expect(screen.getByText("health check failed")).toBeInTheDocument()
    expect(screen.getAllByRole("columnheader")).toHaveLength(9)
    for (const header of screen.getAllByRole("columnheader")) expect(header).toHaveAttribute("scope", "col")
    expect(screen.getByRole("dialog", { name: "Add Worker" })).toBeInTheDocument()
    expect(screen.getByLabelText("Worker name")).toHaveAttribute("autocomplete", "off")
    expect(screen.getByLabelText("Worker API URL")).toHaveAttribute("spellcheck", "false")
    expect(screen.getByLabelText("Compute slots")).toHaveAttribute("max", "65535")
  })

  it("associates worker validation errors and focuses the first invalid field", async () => {
    // Given: the add dialog is open with invalid boundary values.
    const apiClient = createApiClient({
      fetcher: async () => Response.json({ items: [], total: 0 }),
      onUnauthorized: () => undefined,
    })
    render(<WorkersPage apiClient={apiClient} />)
    fireEvent.click(await screen.findByRole("button", { name: "Add Worker" }))
    const nameInput = screen.getByLabelText("Worker name")
    const urlInput = screen.getByLabelText("Worker API URL")
    fireEvent.change(urlInput, { target: { value: "https://worker.example?secret=blocked" } })

    // When: the invalid form is submitted.
    fireEvent.click(screen.getByRole("button", { name: "Save Worker" }))

    // Then: focus moves to the first invalid field and every rendered error is associated.
    await waitFor(() => expect(nameInput).toHaveFocus())
    expect(nameInput).toHaveAttribute("aria-describedby", "worker-name-error")
    expect(urlInput).toHaveAttribute("aria-describedby", "worker-api-url-error")
    expect(screen.getByText("Enter a worker name.")).toHaveAttribute("id", "worker-name-error")
    expect(screen.getByText("Enter a credential-free HTTP(S) base URL without a query or fragment.")).toHaveAttribute("id", "worker-api-url-error")
  })

  it("associates a compute-slot error and focuses its field", async () => {
    // Given: the add dialog contains a valid identity and invalid slot count.
    const apiClient = createApiClient({
      fetcher: async () => Response.json({ items: [], total: 0 }),
      onUnauthorized: () => undefined,
    })
    render(<WorkersPage apiClient={apiClient} />)
    fireEvent.click(await screen.findByRole("button", { name: "Add Worker" }))
    fireEvent.change(screen.getByLabelText("Worker name"), { target: { value: "render-west" } })
    fireEvent.change(screen.getByLabelText("Worker API URL"), { target: { value: "https://worker-west.example" } })
    const slotsInput = screen.getByLabelText("Compute slots")
    fireEvent.change(slotsInput, { target: { value: "0" } })

    // When: the invalid form is submitted.
    fireEvent.click(screen.getByRole("button", { name: "Save Worker" }))

    // Then: the slot field receives focus and owns its announced error.
    await waitFor(() => expect(slotsInput).toHaveFocus())
    expect(slotsInput).toHaveAttribute("aria-describedby", "worker-slots-error")
    expect(screen.getByText("Enter 1 to 65535 compute slots.")).toHaveAttribute("role", "alert")
  })

  it("focuses compute slots for a server field error", async () => {
    // Given: an add form whose valid request receives a server slot error.
    const apiClient = createApiClient({
      fetcher: async (input, init) => {
        const request = new Request(input, init)
        if (request.method === "POST") return Response.json({ error: { code: "invalid_request", message: "invalid worker", retryable: false, field_errors: [{ field: "compute_slots", code: "out_of_range", message: "Compute slots exceed available capacity." }] } }, { status: 400 })
        return Response.json({ items: [], total: 0 })
      },
      onUnauthorized: () => undefined,
    })
    render(<WorkersPage apiClient={apiClient} />)
    fireEvent.click(await screen.findByRole("button", { name: "Add Worker" }))
    fireEvent.change(screen.getByLabelText("Worker name"), { target: { value: "render-west" } })
    fireEvent.change(screen.getByLabelText("Worker API URL"), { target: { value: "https://worker-west.example" } })
    const slotsInput = screen.getByLabelText("Compute slots")

    // When: the valid form is rejected with a slot-specific response.
    fireEvent.click(screen.getByRole("button", { name: "Save Worker" }))

    // Then: the server-invalid slot field receives focus and owns its message.
    await waitFor(() => expect(slotsInput).toHaveFocus())
    expect(slotsInput).toHaveAttribute("aria-describedby", "worker-slots-error")
    expect(screen.getByText("Compute slots exceed available capacity.")).toHaveAttribute("role", "alert")
  })

  it("focuses Add Worker after a successful deletion removes the invoking row", async () => {
    // Given: one deletable worker and an API that removes it after DELETE.
    let deleted = false
    const apiClient = createApiClient({
      fetcher: async (input, init) => {
        const request = new Request(input, init)
        if (request.method === "DELETE") {
          deleted = true
          return Response.json({ worker_id: "550e8400-e29b-41d4-a716-446655440000", deleted: true })
        }
        return Response.json({
          items: deleted ? [] : [{
            id: "550e8400-e29b-41d4-a716-446655440000", version: 4, name: "render-east", api_url: "https://worker.example/api/", enabled: true, online: false, compute_slots: 4,
            capabilities: { workflows: [], refreshed_at: null },
            capacity: { used_slots: 0, available_slots: 4, assigned_tasks: 0, staged_tasks: 0, processing_tasks: 0, active_uploads: 0, active_downloads: 0, progress: null },
            last_seen_at: null, last_assigned_at: null, created_at: "2030-01-01T00:00:00Z", updated_at: "2030-01-01T00:01:00Z", last_error: null,
          }],
          total: deleted ? 0 : 1,
        })
      },
      onUnauthorized: () => undefined,
    })
    render(<WorkersPage apiClient={apiClient} />)
    fireEvent.click(await screen.findByRole("button", { name: "Delete render-east" }))

    // When: deletion succeeds.
    fireEvent.click(screen.getByRole("button", { name: "Delete Worker" }))

    // Then: the removed trigger is gone and focus lands on the stable add action.
    expect(await screen.findByText("No workers registered. Add a worker to make scheduling capacity available.")).toBeInTheDocument()
    await waitFor(() => expect(screen.getByRole("button", { name: "Add Worker" })).toHaveFocus())
  })

  it("contains worker deletion focus and restores the invoking row action", async () => {
    // Given: one worker whose exact row delete action is available.
    const apiClient = createApiClient({
      fetcher: async () => Response.json({
        items: [{
          id: "550e8400-e29b-41d4-a716-446655440000",
          version: 4,
          name: "render-east",
          api_url: "https://worker.example/api/",
          enabled: true,
          online: false,
          compute_slots: 4,
          capabilities: { workflows: [], refreshed_at: null },
          capacity: { used_slots: 0, available_slots: 4, assigned_tasks: 0, staged_tasks: 0, processing_tasks: 0, active_uploads: 0, active_downloads: 0, progress: null },
          last_seen_at: null,
          last_assigned_at: null,
          created_at: "2030-01-01T00:00:00Z",
          updated_at: "2030-01-01T00:01:00Z",
          last_error: null,
        }],
        total: 1,
      }),
      onUnauthorized: () => undefined,
    })
    render(<WorkersPage apiClient={apiClient} />)
    const deleteButton = await screen.findByRole("button", { name: "Delete render-east" })

    // When: deletion is opened, keyboard focus wraps, and Escape cancels.
    fireEvent.click(deleteButton)
    const confirmation = screen.getByRole("alertdialog", { name: "Delete render-east?" })
    const keepWorker = screen.getByRole("button", { name: "Keep Worker" })
    const confirmDelete = screen.getByRole("button", { name: "Delete Worker" })
    await waitFor(() => expect(keepWorker).toHaveFocus())
    fireEvent.keyDown(confirmation, { key: "Tab", shiftKey: true })
    expect(confirmDelete).toHaveFocus()
    fireEvent.keyDown(confirmation, { key: "Tab" })
    expect(keepWorker).toHaveFocus()
    fireEvent.keyDown(confirmation, { key: "Escape" })

    // Then: the modal closes and focus returns to the exact row action.
    expect(screen.queryByRole("alertdialog", { name: "Delete render-east?" })).not.toBeInTheDocument()
    await waitFor(() => expect(deleteButton).toHaveFocus())
  })
})
