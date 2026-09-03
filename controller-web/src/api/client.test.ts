import { describe, expect, it, vi } from "vitest"

import { createApiClient } from "./client"
import { sessionSchema } from "./schemas"

const session = {
  id: "550e8400-e29b-41d4-a716-446655440000",
  authenticated: true,
  method: "session",
  expires_at: "2030-01-01T00:00:00Z",
  idle_expires_at: "2030-01-01T00:00:00Z",
} as const

describe("same-origin API client", () => {
  it("preserves the fetch receiver required by browser-native fetch", async () => {
    // Given: a fetch implementation with the browser-native receiver contract.
    const fetcher: typeof fetch = function (this: unknown): Promise<Response> {
      if (this !== globalThis) return Promise.reject(new TypeError("invalid fetch receiver"))
      return Promise.resolve(
        new Response(JSON.stringify(session), { headers: { "content-type": "application/json" } }),
      )
    }
    const client = createApiClient({ fetcher, onUnauthorized: vi.fn() })

    // When: Ky delegates the session request to the injected fetch implementation.
    const result = await client.request("api/auth/session", { schema: sessionSchema })

    // Then: the typed session crosses the boundary without a receiver failure.
    expect(result).toEqual(session)
  })

  it("parses a successful response and captures rotated CSRF proof", async () => {
    // Given: a valid same-origin session response with a rotated proof.
    const fetcher = vi.fn<typeof fetch>().mockResolvedValue(
      new Response(JSON.stringify(session), {
        headers: { "content-type": "application/json", "x-csrf-token": "rotated-proof" },
      }),
    )
    const client = createApiClient({ fetcher, onUnauthorized: vi.fn() })

    // When: the session boundary is requested.
    const result = await client.request("api/auth/session", { schema: sessionSchema })

    // Then: the typed value is returned and the proof stays in client memory.
    expect(result).toEqual(session)
    expect(client.csrfProof()).toBe("rotated-proof")
    expect(fetcher).toHaveBeenCalledWith(
      expect.objectContaining({ credentials: "same-origin", url: "http://localhost:3000/api/auth/session" }),
    )
  })

  it("attaches CSRF only to same-origin cookie mutations", async () => {
    // Given: a client that received a CSRF proof.
    const requests: Request[] = []
    const fetcher = vi.fn<typeof fetch>().mockImplementation(async (request) => {
      requests.push(request instanceof Request ? request : new Request(request))
      return new Response(JSON.stringify(session), {
        headers: { "content-type": "application/json", "x-csrf-token": "proof" },
      })
    })
    const client = createApiClient({ fetcher, onUnauthorized: vi.fn() })
    await client.request("api/auth/session", { schema: sessionSchema })

    // When: a cookie-authenticated mutation is sent.
    await client.request("api/settings", { method: "PUT", json: {}, schema: sessionSchema })

    // Then: only the mutation carries the in-memory proof and same-origin credentials.
    expect(requests[0]?.headers.has("x-csrf-token")).toBe(false)
    expect(requests[1]?.headers.get("x-csrf-token")).toBe("proof")
    expect(requests[1]?.credentials).toBe("same-origin")
  })

  it.each([
    ["malformed success", new Response("not-json", { status: 200, headers: { "content-type": "application/json" } }), "malformed_response", "malformed_response"],
    ["flat auth", new Response(JSON.stringify({ error: "forbidden" }), { status: 403, headers: { "content-type": "application/json" } }), "forbidden", "forbidden"],
    ["nested operation", new Response(JSON.stringify({ error: { code: "unavailable", message: "remote worker is unavailable", retryable: true, field_errors: [] } }), { status: 503, headers: { "content-type": "application/json" } }), "unavailable", "remote worker is unavailable"],
    ["malformed operation", new Response(JSON.stringify({ error: { code: "unavailable" } }), { status: 503, headers: { "content-type": "application/json" } }), "malformed_response", "malformed_response"],
  ])("returns a recoverable %s error", async (_name, response, code, message) => {
    // Given: an invalid API response.
    const client = createApiClient({ fetcher: vi.fn<typeof fetch>().mockResolvedValue(response), onUnauthorized: vi.fn() })

    // When/Then: the boundary rejects it with a typed recoverable error.
    await expect(client.request("api/auth/session", { schema: sessionSchema })).rejects.toMatchObject({ code, message })
  })

  it("classifies network failure without leaking the low-level message", async () => {
    // Given: a failed network request.
    const client = createApiClient({
      fetcher: vi.fn<typeof fetch>().mockRejectedValue(new TypeError("secret upstream detail")),
      onUnauthorized: vi.fn(),
    })

    // When/Then: the UI receives the stable network classification.
    await expect(client.request("api/auth/session", { schema: sessionSchema })).rejects.toEqual(
      expect.objectContaining({ code: "network_failure" }),
    )
  })

  it("clears CSRF and signals expiry on unauthorized responses", async () => {
    // Given: an authenticated client whose next request expires.
    const onUnauthorized = vi.fn()
    const fetcher = vi.fn<typeof fetch>()
      .mockResolvedValueOnce(new Response(JSON.stringify(session), { headers: { "content-type": "application/json", "x-csrf-token": "proof" } }))
      .mockResolvedValueOnce(new Response(JSON.stringify({ error: "unauthorized" }), { status: 401, headers: { "content-type": "application/json" } }))
    const client = createApiClient({ fetcher, onUnauthorized })
    await client.request("api/auth/session", { schema: sessionSchema })

    // When: the server rejects the session.
    await expect(client.request("api/workers", { schema: sessionSchema })).rejects.toMatchObject({ code: "unauthorized" })

    // Then: no proof remains and the auth owner is notified.
    expect(client.csrfProof()).toBeNull()
    expect(onUnauthorized).toHaveBeenCalledOnce()
  })
})
