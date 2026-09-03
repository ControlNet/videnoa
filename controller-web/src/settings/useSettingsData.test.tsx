import { act, renderHook, waitFor } from "@testing-library/react"
import { describe, expect, it } from "vitest"

import { createApiClient } from "../api/client"
import { useSettingsData } from "./useSettingsData"

const settings = {
  version: 3,
  paths: { input_roots: ["/media/input"], output_roots: ["/media/output"], data_root: "/var/lib/videnoa", temp_root: "/var/tmp/videnoa", password_hash_file: "/run/secrets/password" },
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
      return Response.json(settings)
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
})
