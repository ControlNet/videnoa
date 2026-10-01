import { describe, expect, it, vi } from "vitest"

import { createApiClient } from "./client"
import { apiErrorSchema, sessionSchema } from "./schemas"

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
      return Promise.resolve(new Response(JSON.stringify(session), { headers: { "content-type": "application/json" } }))
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
    expect(fetcher).toHaveBeenCalledWith(expect.objectContaining({ credentials: "same-origin", url: "http://localhost:3000/api/auth/session" }))
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

  it("attaches explicit request headers without persisting them", async () => {
    // Given: a successful task mutation and one request-scoped idempotency key.
    const requests: Request[] = []
    const fetcher = vi.fn<typeof fetch>().mockImplementation(async (request) => {
      requests.push(request instanceof Request ? request : new Request(request))
      return new Response(JSON.stringify(session), { headers: { "content-type": "application/json" } })
    })
    const client = createApiClient({ fetcher, onUnauthorized: vi.fn() })

    // When: the caller supplies the task-ingress header for one request.
    await client.request("api/tasks", {
      method: "POST",
      json: {},
      headers: { "Idempotency-Key": "550e8400-e29b-41d4-a716-446655440001" },
      schema: sessionSchema,
    })

    // Then: the key reaches only the emitted request.
    expect(requests[0]?.headers.get("idempotency-key")).toBe("550e8400-e29b-41d4-a716-446655440001")
    expect(client.csrfProof()).toBeNull()
  })

  it.each([
    [
      "malformed success",
      new Response("not-json", { status: 200, headers: { "content-type": "application/json" } }),
      "malformed_response",
      "malformed_response",
    ],
    [
      "flat auth",
      new Response(JSON.stringify({ error: "forbidden" }), { status: 403, headers: { "content-type": "application/json" } }),
      "forbidden",
      "forbidden",
    ],
    [
      "nested operation",
      new Response(JSON.stringify({ error: { code: "unavailable", message: "remote worker is unavailable", retryable: true, field_errors: [] } }), {
        status: 503,
        headers: { "content-type": "application/json" },
      }),
      "unavailable",
      "remote worker is unavailable",
    ],
    [
      "malformed operation",
      new Response(JSON.stringify({ error: { code: "unavailable" } }), { status: 503, headers: { "content-type": "application/json" } }),
      "malformed_response",
      "malformed_response",
    ],
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
    await expect(client.request("api/auth/session", { schema: sessionSchema })).rejects.toEqual(expect.objectContaining({ code: "network_failure" }))
  })

  it.each(["required", "invalid_value", "unknown_value", "out_of_range", "conflict"] as const)("accepts the Rust %s field error code", (code) => {
    // Given: one exact Rust field error variant.
    const response = { error: { code: "invalid_request", message: "request validation failed", retryable: false, field_errors: [{ field: "output_path", code, message: "invalid" }] } }

    // When: the nested operation error crosses the boundary.
    const parsed = apiErrorSchema.safeParse(response)

    // Then: every closed enum variant is accepted.
    expect(parsed.success).toBe(true)
  })

  it("rejects a field error code outside the Rust enum", () => {
    // Given: a plausible but unsupported field error code.
    const response = { error: { code: "invalid_request", message: "request validation failed", retryable: false, field_errors: [{ field: "output_path", code: "outside_root", message: "invalid" }] } }

    // When: the nested operation error crosses the boundary.
    const parsed = apiErrorSchema.safeParse(response)

    // Then: the strict boundary rejects the invented variant.
    expect(parsed.success).toBe(false)
  })

  describe("CSRF proof rotated by another tab", () => {
    const proofRejected = () =>
      new Response(JSON.stringify({ error: { code: "forbidden", message: "request proof is invalid", retryable: false, field_errors: [] } }), {
        status: 403,
        headers: { "content-type": "application/json" },
      })
    const sessionWithProof = (proof: string) =>
      new Response(JSON.stringify(session), { headers: { "content-type": "application/json", "x-csrf-token": proof } })
    const ok = () => new Response(JSON.stringify(session), { headers: { "content-type": "application/json" } })

    type Sent = { readonly method: string; readonly path: string; readonly proof: string | null }
    function recorder(route: (sent: Sent, index: number) => Promise<Response> | Response) {
      const sent: Sent[] = []
      let calls = 0
      const fetcher = vi.fn<typeof fetch>().mockImplementation(async (input) => {
        const request = input instanceof Request ? input : new Request(input)
        const entry = { method: request.method, path: new URL(request.url).pathname, proof: request.headers.get("x-csrf-token") }
        sent.push(entry)
        return route(entry, calls++)
      })
      return { fetcher, sent }
    }

    async function primedClient(route: (sent: Sent, index: number) => Promise<Response> | Response, onUnauthorized = vi.fn()) {
      const { fetcher, sent } = recorder((entry, index) => (index === 0 ? sessionWithProof("stale") : route(entry, index)))
      const client = createApiClient({ fetcher, onUnauthorized })
      await client.request("api/auth/session", { schema: sessionSchema })
      sent.length = 0
      return { client, sent }
    }

    it("refreshes the proof once and retries a rejected mutation", async () => {
      // Given: a client whose proof was rotated by another tab's session bootstrap.
      const { client, sent } = await primedClient((entry) => {
        if (entry.path === "/api/auth/session") return sessionWithProof("fresh")
        return entry.proof === "fresh" ? ok() : proofRejected()
      })

      // When: the next mutation is rejected with the Controller's proof error.
      const result = await client.request("api/settings", { method: "PUT", json: { a: 1 }, schema: sessionSchema })

      // Then: one session refetch supplies the fresh proof and the retry succeeds transparently.
      expect(result).toEqual(session)
      expect(sent).toEqual([
        { method: "PUT", path: "/api/settings", proof: "stale" },
        { method: "GET", path: "/api/auth/session", proof: null },
        { method: "PUT", path: "/api/settings", proof: "fresh" },
      ])
      expect(client.csrfProof()).toBe("fresh")
    })

    it("surfaces the rejection when the retry is rejected as well", async () => {
      // Given: a Controller that rejects the proof even after a refresh.
      const { client, sent } = await primedClient((entry) => (entry.path === "/api/auth/session" ? sessionWithProof("fresh") : proofRejected()))

      // When/Then: the second rejection reaches the caller and no further retry is attempted.
      await expect(client.request("api/workers/1", { method: "DELETE", schema: sessionSchema })).rejects.toMatchObject({ code: "forbidden", status: 403 })
      expect(sent.map((entry) => entry.method)).toEqual(["DELETE", "GET", "DELETE"])
    })

    it.each([
      ["a read, which carries no proof", "GET", proofRejected],
      [
        "a mutation rejected for another reason",
        "POST",
        () => new Response(JSON.stringify({ error: { code: "not_found", message: "task is gone", retryable: false, field_errors: [] } }), { status: 403, headers: { "content-type": "application/json" } }),
      ],
    ] as const)("does not retry %s", async (_name, method, response) => {
      // Given: a 403 that is not a stale-proof rejection of a mutation.
      const { client, sent } = await primedClient(() => response())

      // When/Then: the error surfaces directly without a session refetch.
      await expect(client.request("api/tasks", { method, schema: sessionSchema })).rejects.toMatchObject({ status: 403 })
      expect(sent).toHaveLength(1)
    })

    it("shares one session refetch between concurrent rejected mutations", async () => {
      // Given: a session refetch that stays pending while a second mutation is rejected.
      let releaseSession: (response: Response) => void = () => {}
      const pendingSession = new Promise<Response>((resolve) => { releaseSession = resolve })
      const { client, sent } = await primedClient((entry) => {
        if (entry.path === "/api/auth/session") return pendingSession
        return entry.proof === "fresh" ? ok() : proofRejected()
      })

      // When: two mutations are rejected while in flight together.
      const first = client.request("api/settings", { method: "PUT", json: {}, schema: sessionSchema })
      const second = client.request("api/workers/1", { method: "DELETE", schema: sessionSchema })
      await vi.waitFor(() => expect(sent.filter((entry) => entry.path === "/api/auth/session")).toHaveLength(1))
      releaseSession(sessionWithProof("fresh"))

      // Then: both retries reuse the single fresh proof.
      await expect(Promise.all([first, second])).resolves.toEqual([session, session])
      expect(sent.filter((entry) => entry.path === "/api/auth/session")).toHaveLength(1)
      expect(sent.filter((entry) => entry.proof === "fresh").map((entry) => entry.method).sort()).toEqual(["DELETE", "PUT"])
    })

    it("treats an expired session discovered during the refresh as unauthorized", async () => {
      // Given: the session itself expired between the rejection and the refresh.
      const onUnauthorized = vi.fn()
      const { client, sent } = await primedClient(
        (entry) =>
          entry.path === "/api/auth/session"
            ? new Response(JSON.stringify({ error: "unauthorized" }), { status: 401, headers: { "content-type": "application/json" } })
            : proofRejected(),
        onUnauthorized,
      )

      // When/Then: the caller sees expiry, the auth owner is told once, and nothing is retried.
      await expect(client.request("api/settings", { method: "PUT", json: {}, schema: sessionSchema })).rejects.toMatchObject({ code: "unauthorized" })
      expect(onUnauthorized).toHaveBeenCalledOnce()
      expect(client.csrfProof()).toBeNull()
      expect(sent.map((entry) => entry.method)).toEqual(["PUT", "GET"])
    })
  })

  it("clears CSRF and signals expiry on unauthorized responses", async () => {
    // Given: an authenticated client whose next request expires.
    const onUnauthorized = vi.fn()
    const fetcher = vi
      .fn<typeof fetch>()
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
