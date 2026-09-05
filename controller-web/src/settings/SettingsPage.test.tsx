import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import { describe, expect, it } from "vitest"

import { createApiClient } from "../api/client"
import { type SettingsResponse, settingsUpdateRequestSchema } from "../api/settingsSchemas"
import { SettingsPage } from "./SettingsPage"

const testOnlySettings = {
  version: 3,
  paths: { workspace: "/synthetic/workspace", data_root: "/synthetic/data", config_file: "/synthetic/controller.toml" },
  server: { host: "0.0.0.0", port: 3001 },
  secure_cookie: true,
  session_absolute_seconds: 86_400,
  session_idle_seconds: 3_600,
  scheduler: { paused: false, default_compute_slots: 2, prefetch_per_worker: 1, max_concurrent_uploads: 2, max_concurrent_downloads: 3 },
  timeouts: { health_seconds: 15, poll_seconds: 5, transfer_seconds: 300 },
  retry: { initial_seconds: 2, maximum_seconds: 30, max_attempts: 4 },
} as const

describe("Settings page", () => {
  it("edits every public configuration group and reports file persistence plus hot apply", async () => {
    // Given: synthetic test-only settings with a server endpoint that can be changed.
    const requests: Request[] = []
    const fetcher: typeof fetch = async (input, init) => {
      const request = new Request(input, init)
      requests.push(request)
      if (new URL(request.url).pathname === "/api/readiness") return Response.json({ status: "ready", checks: [{ name: "persistence", ready: true, message: null }] })
      if (request.method === "PUT") {
        const update = settingsUpdateRequestSchema.parse(await request.clone().json())
        return Response.json({
          ...testOnlySettings,
          version: 4,
          server: update.server,
          secure_cookie: update.auth.secure_cookie,
          session_absolute_seconds: update.auth.session_absolute_seconds,
          session_idle_seconds: update.auth.session_idle_seconds,
          scheduler: update.scheduler,
          timeouts: update.timeouts,
          retry: update.retry,
        })
      }
      if (request.method === "POST") return Response.json({ ...testOnlySettings, version: 4, scheduler: { ...testOnlySettings.scheduler, paused: true } })
      return Response.json(testOnlySettings)
    }
    const apiClient = createApiClient({ fetcher, onUnauthorized: () => undefined })
    render(<SettingsPage apiClient={apiClient} />)

    // When: server, authentication, and scheduler values are changed and saved.
    expect(await screen.findByLabelText("Default compute slots")).toHaveValue(2)
    fireEvent.change(screen.getByLabelText("Server port"), { target: { value: "4555" } })
    fireEvent.click(screen.getByLabelText("Require secure session cookie"))
    fireEvent.change(screen.getByLabelText("Absolute session seconds"), { target: { value: "7200" } })
    fireEvent.change(screen.getByLabelText("Idle session seconds"), { target: { value: "900" } })
    fireEvent.change(screen.getByLabelText("Concurrent uploads"), { target: { value: "5" } })
    fireEvent.click(screen.getByRole("button", { name: "Save and apply settings" }))

    // Then: the exact nested PUT contract is sent and the applied file plus reconnect link are explicit.
    const updateRequest = await waitFor(() => {
      const request = requests.find((candidate) => candidate.method === "PUT")
      expect(request).toBeDefined()
      return request
    })
    expect(await updateRequest?.json()).toEqual({
      version: 3,
      scheduler: { ...testOnlySettings.scheduler, max_concurrent_uploads: 5 },
      timeouts: testOnlySettings.timeouts,
      retry: testOnlySettings.retry,
      server: { host: "0.0.0.0", port: 4555 },
      auth: { secure_cookie: false, session_absolute_seconds: 7200, session_idle_seconds: 900 },
    })
    expect(screen.getByLabelText("Concurrent uploads")).toHaveAttribute("min", "1")
    expect(screen.getByLabelText("Transfer timeout seconds")).toHaveAttribute("max", "604800")
    expect(screen.getByText("/synthetic/workspace")).toBeInTheDocument()
    expect(screen.getByText("/synthetic/controller.toml")).toBeInTheDocument()
    expect(screen.queryByText(/input roots|output roots|temporary root|password hash/i)).not.toBeInTheDocument()
    expect(await screen.findByRole("status")).toHaveTextContent("saved and applied")
    expect(screen.getByRole("link", { name: /open Controller at the new address/i })).toHaveAttribute("href", "http://localhost:4555/")
  })

  it("offers reconnect after a committed degraded endpoint change without reporting file persistence", async () => {
    // Given: a listener update commits but its configuration projection needs repair.
    let currentSettings: SettingsResponse = testOnlySettings
    let settingsCommitted = false
    let mutationCount = 0
    const authoritativeRefetchGate: { resolve?: (response: Response) => void } = {}
    const authoritativeRefetch = new Promise<Response>((resolve) => {
      authoritativeRefetchGate.resolve = resolve
    })
    const fetcher: typeof fetch = async (input, init) => {
      const request = new Request(input, init)
      if (new URL(request.url).pathname === "/api/readiness") return Response.json({ status: "ready", checks: [] })
      if (request.method === "PUT") {
        mutationCount += 1
        const update = settingsUpdateRequestSchema.parse(await request.clone().json())
        currentSettings = {
          ...currentSettings,
          version: currentSettings.version + 1,
          server: update.server,
          secure_cookie: update.auth.secure_cookie,
          session_absolute_seconds: update.auth.session_absolute_seconds,
          session_idle_seconds: update.auth.session_idle_seconds,
          scheduler: update.scheduler,
          timeouts: update.timeouts,
          retry: update.retry,
        }
        settingsCommitted = true
        return Response.json({ error: { code: "unavailable", message: "settings committed and applied; configuration projection repair is pending", retryable: true, field_errors: [] } }, { status: 503 })
      }
      if (request.method === "POST") mutationCount += 1
      if (settingsCommitted) return authoritativeRefetch
      return Response.json(currentSettings)
    }
    const apiClient = createApiClient({ fetcher, onUnauthorized: () => undefined })
    render(<SettingsPage apiClient={apiClient} />)
    const serverPort = await screen.findByLabelText("Server port")
    fireEvent.change(serverPort, { target: { value: "4555" } })

    // When: the operator saves the changed listener address.
    fireEvent.click(screen.getByRole("button", { name: "Save and apply settings" }))

    // Then: the committed endpoint is reachable without a false save receipt.
    expect(await screen.findByRole("alert")).toHaveTextContent("configuration projection repair is pending")
    expect(await screen.findByRole("link", { name: "Open Controller at the new address" })).toHaveAttribute("href", "http://localhost:4555/")
    const pauseButton = screen.getByRole("button", { name: "Pause scheduler" })
    const saveButton = screen.getByRole("button", { name: "Save and apply settings" })
    expect(pauseButton).toBeDisabled()
    expect(saveButton).toBeDisabled()
    fireEvent.click(pauseButton)
    fireEvent.click(saveButton)
    expect(mutationCount).toBe(1)
    if (authoritativeRefetchGate.resolve === undefined) throw new TypeError("Authoritative refetch was not requested")
    authoritativeRefetchGate.resolve(Response.json(currentSettings))
    await waitFor(() => expect(screen.getByText("Settings version 4")).toBeVisible())
    expect(serverPort).toHaveValue(4555)
    expect(screen.getByRole("button", { name: "Save and apply settings" })).toBeEnabled()
    expect(screen.queryByText("Settings saved and applied")).not.toBeInTheDocument()
    expect(screen.queryByText(/Configuration file .* was written/)).not.toBeInTheDocument()
  })

  it("associates numeric errors and focuses the first invalid runtime field", async () => {
    // Given: loaded settings with the first two numeric fields invalid.
    const apiClient = createApiClient({
      fetcher: async (input) => new URL(input instanceof Request ? input.url : input).pathname === "/api/readiness"
        ? Response.json({ status: "ready", checks: [] })
        : Response.json(testOnlySettings),
      onUnauthorized: () => undefined,
    })
    render(<SettingsPage apiClient={apiClient} />)
    const defaultSlots = await screen.findByLabelText("Default compute slots")
    const uploads = screen.getByLabelText("Concurrent uploads")
    fireEvent.change(defaultSlots, { target: { value: "0" } })
    fireEvent.change(uploads, { target: { value: "0" } })
    const form = screen.getByRole("button", { name: "Save and apply settings" }).closest("form")
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
        : Response.json(testOnlySettings),
      onUnauthorized: () => undefined,
    })
    render(<SettingsPage apiClient={apiClient} />)
    const retryInitial = await screen.findByLabelText("Initial retry seconds")
    fireEvent.change(retryInitial, { target: { value: "31" } })
    const form = screen.getByRole("button", { name: "Save and apply settings" }).closest("form")
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
        return Response.json(testOnlySettings)
      },
      onUnauthorized: () => undefined,
    })
    render(<SettingsPage apiClient={apiClient} />)
    const healthTimeout = await screen.findByLabelText("Health timeout seconds")

    // When: the valid form receives a field-specific rejection.
    fireEvent.click(screen.getByRole("button", { name: "Save and apply settings" }))

    // Then: the server-invalid field receives focus and owns the response message.
    await waitFor(() => expect(healthTimeout).toHaveFocus())
    expect(healthTimeout).toHaveAttribute("aria-describedby", "settings-health_seconds-error")
    expect(screen.getByText("Enter a valid health timeout.")).toHaveAttribute("role", "alert")
  })
})
