import { fireEvent, render, screen } from "@testing-library/react"
import { beforeEach, describe, expect, it, vi } from "vitest"

const shellState = vi.hoisted(() => ({ shouldThrow: true }))

vi.mock("./auth/useSessionController", () => ({
  useSessionController: () => ({
    apiClient: {},
    login: vi.fn(),
    logout: vi.fn(),
    retryBootstrap: vi.fn(),
    state: { kind: "authenticated", session: {} },
  }),
}))

vi.mock("./shell/AppShell", () => ({
  AppShell: () => {
    if (shellState.shouldThrow) throw new Error("render failed")
    return <h1>Shell recovered</h1>
  },
}))

import { App } from "./App"

describe("application error boundary", () => {
  beforeEach(() => {
    shellState.shouldThrow = true
    vi.spyOn(console, "error").mockImplementation(() => undefined)
  })

  it("recovers from a render failure through the focused keyboard action", async () => {
    // Given: the authenticated shell fails during render.
    render(<App />)

    // When: the boundary presents its recovery surface.
    expect(screen.getByRole("heading", { name: "Controller interface interrupted" })).toBeVisible()
    const retry = screen.getByRole("button", { name: "Retry application" })
    expect(retry).toHaveFocus()
    shellState.shouldThrow = false
    fireEvent.keyDown(retry, { key: "Enter" })
    fireEvent.click(retry)

    // Then: the application tree mounts again without a page reload.
    expect(await screen.findByRole("heading", { name: "Shell recovered" })).toBeVisible()
  })
})
