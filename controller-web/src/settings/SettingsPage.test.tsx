import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import { describe, expect, it } from "vitest"

import { createApiClient } from "../api/client"
import { SettingsPage } from "./SettingsPage"

describe("Settings page", () => {
  it("separates mutable runtime controls from restart-required configuration", async () => {
    // Given: ready runtime settings and restart-bound paths.
    const requests: Request[] = []
    const fetcher: typeof fetch = async (input, init) => {
      const request = new Request(input, init)
      requests.push(request)
      if (new URL(request.url).pathname === "/api/readiness") return Response.json({ status: "ready", checks: [{ name: "persistence", ready: true, message: null }] })
      return Response.json({
        version: request.method === "POST" ? 4 : 3,
        paths: { input_roots: ["/media/input"], output_roots: ["/media/output"], data_root: "/var/lib/videnoa", temp_root: "/var/tmp/videnoa", password_hash_file: "/run/secrets/password" },
        secure_cookie: true,
        session_absolute_seconds: 86_400,
        session_idle_seconds: 3_600,
        scheduler: { paused: request.method === "POST", default_compute_slots: 2, prefetch_per_worker: 1, max_concurrent_uploads: 2, max_concurrent_downloads: 3 },
        timeouts: { health_seconds: 15, poll_seconds: 5, transfer_seconds: 300 },
        retry: { initial_seconds: 2, maximum_seconds: 30, max_attempts: 4 },
      })
    }
    const apiClient = createApiClient({ fetcher, onUnauthorized: () => undefined })
    render(<SettingsPage apiClient={apiClient} />)

    // When: the settings load and scheduling is paused.
    expect(await screen.findByLabelText("Default compute slots")).toHaveValue(2)
    fireEvent.click(screen.getByRole("button", { name: "Pause scheduler" }))

    // Then: runtime controls are labelled, restart paths are read-only, and pause semantics are explicit.
    await waitFor(() => expect(requests.some((request) => new URL(request.url).pathname === "/api/scheduler/pause")).toBe(true))
    expect(screen.getByLabelText("Concurrent uploads")).toHaveAttribute("min", "1")
    expect(screen.getByLabelText("Transfer timeout seconds")).toHaveAttribute("max", "604800")
    expect(screen.getByText("/var/lib/videnoa")).toBeInTheDocument()
    expect(screen.getByText("/run/secrets/password")).toBeInTheDocument()
    expect(screen.getByRole("region", { name: "Scheduler state" })).toHaveTextContent("Paused")
    expect(screen.queryByLabelText(/password hash file/i)).not.toBeInTheDocument()
  })

  it("associates numeric errors and focuses the first invalid runtime field", async () => {
    // Given: loaded settings with the first two numeric fields invalid.
    const apiClient = createApiClient({
      fetcher: async (input) => new URL(input instanceof Request ? input.url : input).pathname === "/api/readiness"
        ? Response.json({ status: "ready", checks: [] })
        : Response.json({
          version: 3,
          paths: { input_roots: [], output_roots: [], data_root: "/data", temp_root: "/tmp", password_hash_file: "/run/password" },
          secure_cookie: true,
          session_absolute_seconds: 86_400,
          session_idle_seconds: 3_600,
          scheduler: { paused: false, default_compute_slots: 2, prefetch_per_worker: 1, max_concurrent_uploads: 2, max_concurrent_downloads: 3 },
          timeouts: { health_seconds: 15, poll_seconds: 5, transfer_seconds: 300 },
          retry: { initial_seconds: 2, maximum_seconds: 30, max_attempts: 4 },
        }),
      onUnauthorized: () => undefined,
    })
    render(<SettingsPage apiClient={apiClient} />)
    const defaultSlots = await screen.findByLabelText("Default compute slots")
    const uploads = screen.getByLabelText("Concurrent uploads")
    fireEvent.change(defaultSlots, { target: { value: "0" } })
    fireEvent.change(uploads, { target: { value: "0" } })
    const form = screen.getByRole("button", { name: "Save runtime settings" }).closest("form")
    if (!(form instanceof HTMLFormElement)) throw new TypeError("Settings form was not rendered")

    // When: the invalid form is submitted.
    fireEvent.submit(form)

    // Then: focus moves to the first invalid field and errors are programmatically associated.
    await waitFor(() => expect(defaultSlots).toHaveFocus())
    expect(defaultSlots).toHaveAttribute("aria-describedby", "settings-default_compute_slots-error")
    expect(uploads).toHaveAttribute("aria-describedby", "settings-max_concurrent_uploads-error")
    expect(document.getElementById("settings-default_compute_slots-error")).toHaveAttribute("role", "alert")
  })

  it("focuses the initial retry field for the cross-field backoff error", async () => {
    // Given: loaded settings whose initial retry exceeds its maximum.
    const apiClient = createApiClient({
      fetcher: async (input) => new URL(input instanceof Request ? input.url : input).pathname === "/api/readiness"
        ? Response.json({ status: "ready", checks: [] })
        : Response.json({
          version: 3,
          paths: { input_roots: [], output_roots: [], data_root: "/data", temp_root: "/tmp", password_hash_file: "/run/password" },
          secure_cookie: true,
          session_absolute_seconds: 86_400,
          session_idle_seconds: 3_600,
          scheduler: { paused: false, default_compute_slots: 2, prefetch_per_worker: 1, max_concurrent_uploads: 2, max_concurrent_downloads: 3 },
          timeouts: { health_seconds: 15, poll_seconds: 5, transfer_seconds: 300 },
          retry: { initial_seconds: 2, maximum_seconds: 30, max_attempts: 4 },
        }),
      onUnauthorized: () => undefined,
    })
    render(<SettingsPage apiClient={apiClient} />)
    const retryInitial = await screen.findByLabelText("Initial retry seconds")
    fireEvent.change(retryInitial, { target: { value: "31" } })
    const form = screen.getByRole("button", { name: "Save runtime settings" }).closest("form")
    if (!(form instanceof HTMLFormElement)) throw new TypeError("Settings form was not rendered")

    // When: the cross-field-invalid form is submitted.
    fireEvent.submit(form)

    // Then: the initial retry control owns and receives the cross-field error.
    await waitFor(() => expect(retryInitial).toHaveFocus())
    expect(retryInitial).toHaveAttribute("aria-describedby", "settings-initial_seconds-error")
    expect(screen.getByText("Initial delay must not exceed maximum delay.")).toHaveAttribute("role", "alert")
  })

  it("focuses a field identified by a server validation response", async () => {
    // Given: loaded settings and a server field error for the health timeout.
    const apiClient = createApiClient({
      fetcher: async (input, init) => {
        const request = new Request(input, init)
        if (new URL(request.url).pathname === "/api/readiness") return Response.json({ status: "ready", checks: [] })
        if (request.method === "PUT") return Response.json({ error: { code: "invalid_request", message: "invalid settings", retryable: false, field_errors: [{ field: "health_seconds", code: "out_of_range", message: "Enter a valid health timeout." }] } }, { status: 400 })
        return Response.json({
          version: 3,
          paths: { input_roots: [], output_roots: [], data_root: "/data", temp_root: "/tmp", password_hash_file: "/run/password" },
          secure_cookie: true,
          session_absolute_seconds: 86_400,
          session_idle_seconds: 3_600,
          scheduler: { paused: false, default_compute_slots: 2, prefetch_per_worker: 1, max_concurrent_uploads: 2, max_concurrent_downloads: 3 },
          timeouts: { health_seconds: 15, poll_seconds: 5, transfer_seconds: 300 },
          retry: { initial_seconds: 2, maximum_seconds: 30, max_attempts: 4 },
        })
      },
      onUnauthorized: () => undefined,
    })
    render(<SettingsPage apiClient={apiClient} />)
    const healthTimeout = await screen.findByLabelText("Health timeout seconds")

    // When: the valid form receives a field-specific rejection.
    fireEvent.click(screen.getByRole("button", { name: "Save runtime settings" }))

    // Then: the server-invalid field receives focus and owns the response message.
    await waitFor(() => expect(healthTimeout).toHaveFocus())
    expect(healthTimeout).toHaveAttribute("aria-describedby", "settings-health_seconds-error")
    expect(screen.getByText("Enter a valid health timeout.")).toHaveAttribute("role", "alert")
  })
})
