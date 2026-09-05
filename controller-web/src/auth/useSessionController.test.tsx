import { act, renderHook, waitFor } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"

import { setupRequestSchema } from "../api/schemas"
import { useSessionController } from "./useSessionController"

const setupRaceNotice = "Controller setup was completed elsewhere. Sign in with the administrator password."

function response(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  })
}

describe("setup race recovery", () => {
  afterEach(() => vi.unstubAllGlobals())

  it("returns an unauthenticated result when another client completes setup first", async () => {
    // Given: setup is initially available, then another synthetic test-only client wins the race.
    let setupChecks = 0
    vi.stubGlobal("fetch", vi.fn<typeof fetch>((input, init) => {
      const request = new Request(input, init)
      const path = new URL(request.url).pathname
      if (path === "/api/auth/setup" && request.method === "GET") {
        setupChecks += 1
        return Promise.resolve(response({ initialized: setupChecks > 1 }))
      }
      if (path === "/api/auth/setup" && request.method === "POST") {
        return Promise.resolve(response({ error: "conflict" }, 409))
      }
      if (path === "/api/auth/session") {
        return Promise.resolve(response({ error: "unauthorized" }, 401))
      }
      return Promise.resolve(response({ error: "internal" }, 500))
    }))
    const { result } = renderHook(() => useSessionController())
    await waitFor(() => expect(result.current.state.kind).toBe("setup_required"))

    // When: this client submits after setup completed elsewhere.
    const setupResult = await act(() => result.current.setup(setupRequestSchema.parse(Object.fromEntries([
      ["password", "synthetic-passphrase"],
      ["password_confirmation", "synthetic-passphrase"],
    ]))))

    // Then: the callback and controller state both report login-required recovery, not setup success.
    expect(setupResult).toEqual({ ok: false, message: setupRaceNotice })
    expect(result.current.state).toEqual({ kind: "unauthenticated", notice: setupRaceNotice })
  })
})
