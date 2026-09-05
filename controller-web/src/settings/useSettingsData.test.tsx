import { act, renderHook, waitFor } from "@testing-library/react"
import { describe, expect, it } from "vitest"

import { createApiClient } from "../api/client"
import { useSettingsData } from "./useSettingsData"

const testOnlySettings = {
  version: 3,
  paths: { workspace: "/synthetic/workspace", data_root: "/synthetic/data", config_file: "/synthetic/controller.toml" },
  server: { host: "127.0.0.1", port: 3001 },
  secure_cookie: true,
  session_absolute_seconds: 86_400,
  session_idle_seconds: 3_600,
  scheduler: { paused: false, default_compute_slots: 2, prefetch_per_worker: 1, max_concurrent_uploads: 2, max_concurrent_downloads: 3 },
  timeouts: { health_seconds: 15, poll_seconds: 5, transfer_seconds: 300 },
  retry: { initial_seconds: 2, maximum_seconds: 30, max_attempts: 4 },
} as const

describe("settings data requests", () => {
  it("submits the displayed version and refetches after a stale pause", async () => {
    // Given: loaded settings whose pause mutation conflicts.
    const requests: Request[] = []
    const fetcher: typeof fetch = async (input, init) => {
      const request = new Request(input, init)
      requests.push(request)
      if (request.method === "POST") {
        return Response.json({ error: { code: "conflict", message: "settings changed since they were read", retryable: false, field_errors: [] } }, { status: 409 })
      }
      if (new URL(request.url).pathname === "/api/readiness") return Response.json({ status: "ready", checks: [] })
      return Response.json(testOnlySettings)
    }
    const apiClient = createApiClient({ fetcher, onUnauthorized: () => undefined })
    const { result } = renderHook(() => useSettingsData(apiClient))
    await waitFor(() => expect(result.current.settings?.version).toBe(3))

    // When: the operator pauses scheduling.
    await act(async () => result.current.setPaused(true))

    // Then: the version is submitted and both authoritative read endpoints refresh.
    await waitFor(() => expect(requests.filter((request) => request.method === "GET")).toHaveLength(4))
    expect(await requests.find((request) => request.method === "POST")?.json()).toEqual({ version: 3 })
    expect(result.current.actionError?.code).toBe("conflict")
  })

  it("refetches authoritative settings when a committed projection needs repair", async () => {
    // Given: a save commits version four but reports a retryable projection failure.
    const requests: Request[] = []
    let settingsCommitted = false
    const fetcher: typeof fetch = async (input, init) => {
      const request = new Request(input, init)
      requests.push(request)
      if (request.method === "PUT") {
        settingsCommitted = true
        return Response.json({
          error: {
            code: "unavailable",
            message: "settings committed and applied; configuration projection repair is pending",
            retryable: true,
            field_errors: [],
          },
        }, { status: 503 })
      }
      if (new URL(request.url).pathname === "/api/readiness") return Response.json({ status: "ready", checks: [] })
      return Response.json(settingsCommitted
        ? { ...testOnlySettings, version: 4, server: { ...testOnlySettings.server, port: 4555 } }
        : testOnlySettings)
    }
    const apiClient = createApiClient({ fetcher, onUnauthorized: () => undefined })
    const { result } = renderHook(() => useSettingsData(apiClient))
    await waitFor(() => expect(result.current.settings?.version).toBe(3))

    // When: the operator saves the new listener address.
    await act(async () => result.current.save({
      version: 3,
      server: { host: "127.0.0.1", port: 4555 },
      auth: { secure_cookie: true, session_absolute_seconds: 86_400, session_idle_seconds: 3_600 },
      scheduler: testOnlySettings.scheduler,
      timeouts: testOnlySettings.timeouts,
      retry: testOnlySettings.retry,
    }))

    // Then: the committed version replaces the stale form while the degradation remains visible.
    await waitFor(() => expect(result.current.settings?.version).toBe(4))
    expect(result.current.settings?.server.port).toBe(4555)
    expect(result.current.actionError?.code).toBe("unavailable")
    expect(result.current.actionError?.retryable).toBe(true)
    expect(requests.filter((request) => request.method === "GET")).toHaveLength(4)
  })
})
