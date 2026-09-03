import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"

import { App } from "../App"

const session = {
  id: "550e8400-e29b-41d4-a716-446655440000",
  authenticated: true,
  method: "session",
  expires_at: "2030-01-01T00:00:00Z",
  idle_expires_at: "2030-01-01T00:00:00Z",
} as const

class FakeEventSource extends EventTarget {
  static latest: FakeEventSource | null = null
  static readonly CONNECTING = 0
  static readonly OPEN = 1
  static readonly CLOSED = 2

  readonly CONNECTING = FakeEventSource.CONNECTING
  readonly OPEN = FakeEventSource.OPEN
  readonly CLOSED = FakeEventSource.CLOSED
  readonly url: string
  readonly withCredentials: boolean
  readyState = FakeEventSource.CONNECTING

  constructor(url: string | URL, init?: EventSourceInit) {
    super()
    this.url = String(url)
    this.withCredentials = init?.withCredentials ?? false
    FakeEventSource.latest = this
  }

  close(): void {
    this.readyState = FakeEventSource.CLOSED
  }

  open(): void {
    this.readyState = FakeEventSource.OPEN
    this.dispatchEvent(new Event("open"))
  }

  fail(readyState: number): void {
    this.readyState = readyState
    this.dispatchEvent(new Event("error"))
  }
}

function response(body: unknown, status = 200, csrf?: string): Response {
  const headers = new Headers({ "content-type": "application/json" })
  if (csrf !== undefined) headers.set("x-csrf-token", csrf)
  return new Response(JSON.stringify(body), { status, headers })
}

describe("authenticated Controller shell", () => {
  afterEach(() => {
    FakeEventSource.latest = null
    vi.unstubAllGlobals()
    window.history.replaceState({}, "", "/")
  })

  it("protects a deep link and shows only the accessible login form", async () => {
    // Given: no valid cookie session on a protected route.
    window.history.replaceState({}, "", "/workers")
    vi.stubGlobal("fetch", vi.fn<typeof fetch>().mockResolvedValue(response({ error: "unauthorized" }, 401)))

    // When: the app bootstraps.
    render(<App />)

    // Then: login is the only application surface and focus reaches the password field.
    expect(await screen.findByRole("heading", { name: "Sign in to Controller" })).toBeVisible()
    expect(screen.queryByRole("navigation", { name: "Primary" })).not.toBeInTheDocument()
    await waitFor(() => expect(screen.getByLabelText("Controller password")).toHaveFocus())
  })

  it("bootstraps an existing cookie session directly into the requested route", async () => {
    // Given: a valid HttpOnly-cookie session and a deep link.
    window.history.replaceState({}, "", "/settings")
    vi.stubGlobal("fetch", vi.fn<typeof fetch>().mockResolvedValue(response(session, 200, "proof")))

    // When: passive session bootstrap succeeds.
    render(<App />)

    // Then: the compact shell preserves the route and marks navigation active.
    expect(await screen.findByRole("heading", { name: "Settings" })).toBeVisible()
    expect(screen.getByRole("link", { name: "Settings" })).toHaveAttribute("aria-current", "page")
    expect(screen.getByRole("main")).toHaveFocus()
  })

  it("shows the accurate implementation owner for every placeholder route", async () => {
    // Given: an authenticated Controller shell.
    vi.stubGlobal("fetch", vi.fn<typeof fetch>().mockResolvedValue(response(session, 200, "proof")))
    render(<App />)
    await screen.findByRole("heading", { name: "Tasks" })

    // When/Then: each route names the task that owns its domain implementation.
    expect(screen.getByText("TASK 16")).toBeVisible()
    fireEvent.click(screen.getByRole("link", { name: "Settings" }))
    expect(screen.getByText("TASK 18")).toBeVisible()
    fireEvent.click(screen.getByRole("link", { name: "Workers" }))
    expect(screen.getByText("TASK 18")).toBeVisible()
  })

  it("logs in, navigates by links, and logs out without browser storage", async () => {
    // Given: an unauthenticated bootstrap followed by successful login and logout.
    const fetcher = vi.fn<typeof fetch>()
      .mockResolvedValueOnce(response({ error: "unauthorized" }, 401))
      .mockResolvedValueOnce(response({ session }, 200, "proof"))
      .mockResolvedValueOnce(response({ logged_out: true }))
    vi.stubGlobal("fetch", fetcher)
    render(<App />)
    const password = await screen.findByLabelText("Controller password")

    // When: credentials are submitted, navigation is used, then logout is requested.
    fireEvent.change(password, { target: { value: "transient-password" } })
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }))
    expect(await screen.findByRole("heading", { name: "Tasks" })).toBeVisible()
    fireEvent.click(screen.getByRole("link", { name: "Workers" }))
    expect(await screen.findByRole("heading", { name: "Workers" })).toBeVisible()
    fireEvent.click(screen.getByRole("button", { name: "Sign out" }))

    // Then: auth state clears, login returns, and storage remains empty.
    expect(await screen.findByRole("heading", { name: "Sign in to Controller" })).toBeVisible()
    expect(localStorage).toHaveLength(0)
    expect(sessionStorage).toHaveLength(0)
  })

  it.each([
    ["wrong password", response({ error: "unauthorized" }, 401), "The password was not accepted."],
    ["malformed response", new Response("not-json", { status: 200, headers: { "content-type": "application/json" } }), "Controller returned an invalid response."],
  ])("shows a recoverable summary for %s", async (_name, loginResponse, message) => {
    // Given: login starts unauthenticated and the mutation fails recoverably.
    const fetcher = vi.fn<typeof fetch>()
      .mockResolvedValueOnce(response({ error: "unauthorized" }, 401))
      .mockResolvedValueOnce(loginResponse)
    vi.stubGlobal("fetch", fetcher)
    render(<App />)
    fireEvent.change(await screen.findByLabelText("Controller password"), { target: { value: "wrong" } })

    // When: login is submitted.
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }))

    // Then: the form remains usable and the error summary receives focus.
    const alert = await screen.findByRole("alert")
    expect(alert).toHaveTextContent(message)
    expect(alert).toHaveFocus()
    expect(screen.getByRole("button", { name: "Sign in" })).toBeEnabled()
  })

  it("recovers from a session bootstrap network failure", async () => {
    // Given: Controller cannot be reached during bootstrap.
    const fetcher = vi.fn<typeof fetch>()
      .mockRejectedValueOnce(new TypeError("offline"))
      .mockResolvedValueOnce(response({ error: "unauthorized" }, 401))
    vi.stubGlobal("fetch", fetcher)
    render(<App />)

    // When: the operator retries.
    expect(await screen.findByRole("alert")).toHaveTextContent("Controller could not be reached.")
    fireEvent.click(screen.getByRole("button", { name: "Retry session check" }))

    // Then: recovery returns to login rather than crashing.
    await waitFor(() => expect(screen.getByRole("heading", { name: "Sign in to Controller" })).toBeVisible())
  })

  it("keeps the authenticated shell operable when logout fails and allows retry", async () => {
    // Given: an authenticated shell whose first logout request fails.
    const fetcher = vi.fn<typeof fetch>()
      .mockResolvedValueOnce(response(session, 200, "proof"))
      .mockResolvedValueOnce(response({ error: { code: "unavailable", message: "remote worker is unavailable", retryable: true, field_errors: [] } }, 503))
      .mockResolvedValueOnce(response({ logged_out: true }))
    vi.stubGlobal("fetch", fetcher)
    render(<App />)
    await screen.findByRole("heading", { name: "Tasks" })

    // When: sign out fails at the Controller boundary.
    fireEvent.click(screen.getByRole("button", { name: "Sign out" }))

    // Then: the shell remains, the error receives focus, and a second attempt can succeed.
    const alert = await screen.findByRole("alert")
    expect(alert).toHaveTextContent("Controller could not complete sign out. Try again.")
    expect(alert).toHaveFocus()
    expect(screen.getByRole("button", { name: "Sign out" })).toBeEnabled()
    fireEvent.click(screen.getByRole("button", { name: "Sign out" }))
    expect(await screen.findByRole("heading", { name: "Sign in to Controller" })).toBeVisible()
  })

  it("reports the actual event-stream lifecycle", async () => {
    // Given: an authenticated shell with a controllable EventSource.
    vi.stubGlobal("EventSource", FakeEventSource)
    vi.stubGlobal("fetch", vi.fn<typeof fetch>().mockResolvedValue(response(session, 200, "proof")))
    render(<App />)
    await screen.findByRole("heading", { name: "Tasks" })

    // When/Then: connection events produce honest, explicit status labels.
    expect(screen.getByText("Controller connecting")).toBeVisible()
    FakeEventSource.latest?.open()
    expect(await screen.findByText("Controller connected")).toBeVisible()
    FakeEventSource.latest?.fail(FakeEventSource.CONNECTING)
    expect(await screen.findByText("Controller reconnecting")).toBeVisible()
    FakeEventSource.latest?.fail(FakeEventSource.CLOSED)
    expect(await screen.findByText("Controller unavailable")).toBeVisible()
  })
})
