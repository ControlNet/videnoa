import { act, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import { App } from "../App"

const session = {
  id: "550e8400-e29b-41d4-a716-446655440000",
  authenticated: true,
  method: "session",
  expires_at: "2030-01-01T00:00:00Z",
  idle_expires_at: "2030-01-01T00:00:00Z",
} as const

const emptyTaskPage = { items: [], total: 0, limit: 50, offset: 0 }
const emptyTaskCounts = {
  items: [
    "queued", "reserved", "uploading", "staged", "submitting", "processing",
    "remote_completed", "downloading", "verifying", "publishing", "remote_cleanup",
    "completed", "failed", "cancelled",
  ].map((status) => ({ status, count: 0 })),
  total: 0,
}
const emptyWorkerList = { items: [], total: 0 }
const initializedSetup = { initialized: true } as const
const pendingSetup = { initialized: false } as const

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

function pathFor(input: RequestInfo | URL): string {
  const value = input instanceof Request ? input.url : String(input)
  return new URL(value, window.location.origin).pathname
}

function authenticatedFetcher(): ReturnType<typeof vi.fn<typeof fetch>> {
  return vi.fn<typeof fetch>((input) => {
    switch (pathFor(input)) {
      case "/api/auth/setup": return Promise.resolve(response(initializedSetup))
      case "/api/auth/session": return Promise.resolve(response(session, 200, "proof"))
      case "/api/tasks": return Promise.resolve(response(emptyTaskPage))
      case "/api/status-counts": return Promise.resolve(response(emptyTaskCounts))
      default: return Promise.resolve(response({ error: "internal" }, 500))
    }
  })
}

function deferred<T>(): { readonly promise: Promise<T>; readonly resolve: (value: T) => void } {
  let resolvePromise: ((value: T) => void) | null = null
  const promise = new Promise<T>((resolve) => {
    resolvePromise = resolve
  })
  return {
    promise,
    resolve: (value) => {
      if (resolvePromise === null) throw new TypeError("Deferred promise was not initialized")
      resolvePromise(value)
    },
  }
}

describe("authenticated Controller shell", () => {
  beforeEach(() => {
    vi.stubGlobal("ResizeObserver", class {
      observe(): void {}
      unobserve(): void {}
      disconnect(): void {}
    })
  })

  afterEach(() => {
    FakeEventSource.latest = null
    vi.unstubAllGlobals()
    window.history.replaceState({}, "", "/")
  })

  it("protects a deep link and shows only the accessible login form", async () => {
    // Given: no valid cookie session on a protected route.
    window.history.replaceState({}, "", "/workers")
    vi.stubGlobal("fetch", vi.fn<typeof fetch>((input) => {
      switch (pathFor(input)) {
        case "/api/auth/setup": return Promise.resolve(response(initializedSetup))
        case "/api/auth/session": return Promise.resolve(response({ error: "unauthorized" }, 401))
        default: return Promise.resolve(response({ error: "internal" }, 500))
      }
    }))

    // When: the app bootstraps.
    render(<App />)

    // Then: login is the only application surface and focus reaches the password field.
    expect(await screen.findByRole("heading", { name: "Sign in to Controller" })).toBeVisible()
    expect(screen.queryByRole("navigation", { name: "Primary" })).not.toBeInTheDocument()
    await waitFor(() => expect(screen.getByLabelText("Controller password")).toHaveFocus())
  })

  it("checks initialization before session and offers first-run password setup", async () => {
    // Given: a synthetic test-only Controller that has not been initialized.
    const requestedPaths: string[] = []
    vi.stubGlobal("fetch", vi.fn<typeof fetch>((input) => {
      const path = pathFor(input)
      requestedPaths.push(path)
      if (path === "/api/auth/setup") return Promise.resolve(response(pendingSetup))
      return Promise.resolve(response({ error: "internal" }, 500))
    }))

    // When: the application bootstraps for the first time.
    render(<App />)

    // Then: setup is the only surface, setup ran before session, and focus reaches the new password.
    expect(await screen.findByRole("heading", { name: "Set up Controller access" })).toBeVisible()
    expect(requestedPaths).toEqual(["/api/auth/setup"])
    expect(screen.queryByRole("heading", { name: "Sign in to Controller" })).not.toBeInTheDocument()
    await waitFor(() => expect(screen.getByLabelText("Create password")).toHaveFocus())
  })

  it.each([
    ["undersized password", "short", "short", "Use at least 12 UTF-8 bytes.", "Create password"],
    ["oversized multibyte password", "界".repeat(342), "界".repeat(342), "Use at most 1024 UTF-8 bytes.", "Create password"],
    ["mismatched confirmation", "synthetic-passphrase", "different-passphrase", "Passwords do not match.", "Confirm password"],
  ] as const)("rejects a locally %s", async (_case, passwordValue, confirmationValue, message, focusedLabel) => {
    // Given: a synthetic test-only uninitialized Controller and one invalid setup pair.
    const requests: Request[] = []
    const fetcher = vi.fn<typeof fetch>(async (input, init) => {
      const request = new Request(input, init)
      requests.push(request)
      return response(pendingSetup)
    })
    vi.stubGlobal("fetch", fetcher)
    render(<App />)
    const password = await screen.findByLabelText("Create password")
    const confirmation = screen.getByLabelText("Confirm password")

    // When: the invalid pair is submitted.
    fireEvent.change(password, { target: { value: passwordValue } })
    fireEvent.change(confirmation, { target: { value: confirmationValue } })
    fireEvent.click(screen.getByRole("button", { name: "Create secure access" }))

    // Then: the precise field error receives focus and transport never sees the secret.
    expect(await screen.findByText(message)).toHaveAttribute("role", "alert")
    expect(screen.getByLabelText(focusedLabel)).toHaveFocus()
    expect(requests).toHaveLength(1)
  })

  it("authenticates setup without browser-stored secrets", async () => {
    // Given: a synthetic test-only uninitialized Controller and a successful setup boundary.
    const requests: Request[] = []
    vi.stubGlobal("fetch", vi.fn<typeof fetch>(async (input, init) => {
      const request = new Request(input, init)
      requests.push(request)
      switch (new URL(request.url).pathname) {
        case "/api/auth/setup": return request.method === "GET" ? response(pendingSetup) : response({ session }, 200, "setup-proof")
        case "/api/tasks": return response(emptyTaskPage)
        case "/api/status-counts": return response(emptyTaskCounts)
        default: return response({ error: "internal" }, 500)
      }
    }))
    render(<App />)
    fireEvent.change(await screen.findByLabelText("Create password"), { target: { value: "synthetic-passphrase" } })
    fireEvent.change(screen.getByLabelText("Confirm password"), { target: { value: "synthetic-passphrase" } })

    // When: the valid setup pair is submitted.
    fireEvent.click(screen.getByRole("button", { name: "Create secure access" }))

    // Then: the exact request enters the authenticated shell and no browser storage is used.
    expect(await screen.findByRole("heading", { name: "Tasks" })).toBeVisible()
    const setupRequest = requests.find((request) => new URL(request.url).pathname === "/api/auth/setup" && request.method === "POST")
    expect(await setupRequest?.json()).toEqual(Object.fromEntries([
      ["password", "synthetic-passphrase"],
      ["password_confirmation", "synthetic-passphrase"],
    ]))
    expect(localStorage).toHaveLength(0)
    expect(sessionStorage).toHaveLength(0)
  })

  it("recovers an already-initialized setup race into normal sign-in", async () => {
    // Given: another synthetic test-only client initializes the Controller before this form submits.
    let setupChecks = 0
    const requestedPaths: string[] = []
    vi.stubGlobal("fetch", vi.fn<typeof fetch>((input, init) => {
      const request = new Request(input, init)
      const path = new URL(request.url).pathname
      requestedPaths.push(`${request.method} ${path}`)
      if (path === "/api/auth/setup" && request.method === "GET") {
        setupChecks += 1
        return Promise.resolve(response({ initialized: setupChecks > 1 }))
      }
      if (path === "/api/auth/setup" && request.method === "POST") {
        return Promise.resolve(response({ error: "conflict" }, 409))
      }
      if (path === "/api/auth/session") return Promise.resolve(response({ error: "unauthorized" }, 401))
      return Promise.resolve(response({ error: "internal" }, 500))
    }))
    render(<App />)
    fireEvent.change(await screen.findByLabelText("Create password"), { target: { value: "synthetic-passphrase" } })
    fireEvent.change(screen.getByLabelText("Confirm password"), { target: { value: "synthetic-passphrase" } })

    // When: this client submits after initialization completed elsewhere.
    fireEvent.click(screen.getByRole("button", { name: "Create secure access" }))

    // Then: setup and session are rechecked in order and the ordinary login surface is restored.
    expect(await screen.findByRole("heading", { name: "Sign in to Controller" })).toBeVisible()
    expect(screen.getByRole("status")).toHaveTextContent("Controller setup was completed elsewhere. Sign in with the administrator password.")
    expect(requestedPaths).toEqual([
      "GET /api/auth/setup",
      "POST /api/auth/setup",
      "GET /api/auth/setup",
      "GET /api/auth/session",
    ])
    expect(screen.getByLabelText("Controller password")).toHaveFocus()
  })

  it("bootstraps an existing cookie session directly into the requested route", async () => {
    // Given: a valid HttpOnly-cookie session and a deep link.
    window.history.replaceState({}, "", "/settings")
    vi.stubGlobal("fetch", authenticatedFetcher())

    // When: passive session bootstrap succeeds.
    render(<App />)

    // Then: the compact shell preserves the route and marks navigation active.
    expect(await screen.findByRole("heading", { name: "Settings" })).toBeVisible()
    expect(screen.getByRole("link", { name: "Settings" })).toHaveAttribute("aria-current", "page")
    expect(screen.getByRole("main")).toHaveFocus()
  })

  it("renders every delivered operational route without placeholder ownership", async () => {
    // Given: an authenticated Controller shell.
    vi.stubGlobal("fetch", authenticatedFetcher())
    render(<App />)
    await screen.findByRole("heading", { name: "Tasks" })

    // When/Then: Tasks, Settings, and Workers each render their delivered operational surface.
    expect(screen.getByRole("table")).toBeVisible()
    fireEvent.click(screen.getByRole("link", { name: "Settings" }))
    expect(screen.getByRole("heading", { name: "Settings" })).toBeVisible()
    fireEvent.click(screen.getByRole("link", { name: "Workers" }))
    expect(screen.getByRole("heading", { name: "Workers" })).toBeVisible()
    expect(screen.queryByText("TASK 18")).not.toBeInTheDocument()
  })

  it("logs in, navigates by links, and logs out without browser storage", async () => {
    // Given: an unauthenticated bootstrap followed by successful login and logout.
    const fetcher = vi.fn<typeof fetch>((input) => {
      switch (pathFor(input)) {
        case "/api/auth/setup": return Promise.resolve(response(initializedSetup))
        case "/api/auth/session": return Promise.resolve(response({ error: "unauthorized" }, 401))
        case "/api/auth/login": return Promise.resolve(response({ session }, 200, "proof"))
        case "/api/tasks": return Promise.resolve(response(emptyTaskPage))
        case "/api/status-counts": return Promise.resolve(response(emptyTaskCounts))
        case "/api/auth/logout": return Promise.resolve(response({ logged_out: true }))
        default: return Promise.resolve(response({ error: "internal" }, 500))
      }
    })
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
    ["wrong password", response({ error: "unauthorized" }, 401), "The password was not accepted.", true],
    ["malformed response", new Response("not-json", { status: 200, headers: { "content-type": "application/json" } }), "Controller returned an invalid response.", false],
  ])("shows a recoverable summary for %s", async (_name, loginResponse, message, passwordInvalid) => {
    // Given: login starts unauthenticated and the mutation fails recoverably.
    const fetcher = vi.fn<typeof fetch>()
      .mockResolvedValueOnce(response(initializedSetup))
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
    const password = screen.getByLabelText("Controller password")
    if (passwordInvalid) {
      expect(password).toHaveAttribute("aria-invalid", "true")
      expect(password).toHaveAttribute("aria-describedby", alert.id)
    } else {
      expect(password).not.toHaveAttribute("aria-invalid")
      expect(password).not.toHaveAttribute("aria-describedby")
    }
    expect(screen.getByRole("button", { name: "Sign in" })).toBeEnabled()
  })

  it("refocuses the current malformed-response generation and permits recovery", async () => {
    // Given: two malformed login responses followed by a successful retry.
    let loginAttempts = 0
    const fetcher = vi.fn<typeof fetch>((input) => {
      switch (pathFor(input)) {
        case "/api/auth/setup": return Promise.resolve(response(initializedSetup))
        case "/api/auth/session": return Promise.resolve(response({ error: "unauthorized" }, 401))
        case "/api/auth/login": {
          loginAttempts += 1
          return loginAttempts < 3
            ? Promise.resolve(new Response("not-json", { status: 200, headers: { "content-type": "application/json" } }))
            : Promise.resolve(response({ session }, 200, "proof"))
        }
        case "/api/tasks": return Promise.resolve(response(emptyTaskPage))
        case "/api/status-counts": return Promise.resolve(response(emptyTaskCounts))
        case "/api/workers": return Promise.resolve(response(emptyWorkerList))
        default: return Promise.resolve(response({ error: "internal" }, 500))
      }
    })
    vi.stubGlobal("fetch", fetcher)
    window.history.replaceState({}, "", "/workers")
    render(<App />)
    const password = await screen.findByLabelText("Controller password")
    fireEvent.change(password, { target: { value: "synthetic-retry-value" } })

    // When: the same recoverable failure is retried after focus moves back to the field.
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }))
    const firstAlert = await screen.findByRole("alert")
    expect(firstAlert).toHaveFocus()
    password.focus()
    fireEvent.change(password, { target: { value: "synthetic-retry-value-updated" } })
    expect(password).toHaveFocus()
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }))

    // Then: the current summary owns focus again and a later success retains route focus behavior.
    const secondAlert = await screen.findByRole("alert")
    expect(secondAlert).toHaveFocus()
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }))
    expect(await screen.findByRole("heading", { name: "Workers" })).toBeVisible()
    expect(screen.getByRole("main")).toHaveFocus()
  })

  it("does not apply malformed-response focus after the login page unmounts", async () => {
    // Given: an in-flight login request and an unrelated focus target outside the app.
    const loginResponse = deferred<Response>()
    const fetcher = vi.fn<typeof fetch>()
      .mockResolvedValueOnce(response(initializedSetup))
      .mockResolvedValueOnce(response({ error: "unauthorized" }, 401))
      .mockReturnValueOnce(loginResponse.promise)
    vi.stubGlobal("fetch", fetcher)
    const rendered = render(<App />)
    fireEvent.change(await screen.findByLabelText("Controller password"), { target: { value: "synthetic-stale-value" } })
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }))
    const outsideTarget = document.createElement("button")
    document.body.append(outsideTarget)
    rendered.unmount()
    outsideTarget.focus()

    // When: the stale request completes with a malformed response after unmount.
    await act(async () => loginResponse.resolve(new Response("not-json", { status: 200, headers: { "content-type": "application/json" } })))

    // Then: no stale summary is rendered and unrelated focus is unchanged.
    expect(screen.queryByRole("alert")).not.toBeInTheDocument()
    expect(outsideTarget).toHaveFocus()
    outsideTarget.remove()
  })

  it("recovers from a session bootstrap network failure", async () => {
    // Given: Controller cannot be reached during bootstrap.
    const fetcher = vi.fn<typeof fetch>()
      .mockRejectedValueOnce(new TypeError("offline"))
      .mockResolvedValueOnce(response(initializedSetup))
      .mockResolvedValueOnce(response({ error: "unauthorized" }, 401))
    vi.stubGlobal("fetch", fetcher)
    render(<App />)

    // When: the operator retries.
    expect(await screen.findByRole("alert")).toHaveTextContent("Controller could not be reached.")
    fireEvent.click(screen.getByRole("button", { name: "Retry Controller check" }))

    // Then: recovery returns to login rather than crashing.
    await waitFor(() => expect(screen.getByRole("heading", { name: "Sign in to Controller" })).toBeVisible())
  })

  it("focuses a malformed session bootstrap summary and recovers on retry", async () => {
    // Given: session bootstrap returns malformed JSON before a retry reports no session.
    const fetcher = vi.fn<typeof fetch>()
      .mockResolvedValueOnce(new Response("not-json", { status: 200, headers: { "content-type": "application/json" } }))
      .mockResolvedValueOnce(response(initializedSetup))
      .mockResolvedValueOnce(response({ error: "unauthorized" }, 401))
    vi.stubGlobal("fetch", fetcher)
    render(<App />)

    // When: the malformed bootstrap response is committed and the operator retries.
    const alert = await screen.findByRole("alert")

    // Then: the summary owns focus before retry, and login restores normal initial field focus.
    expect(alert).toHaveTextContent("Controller returned an invalid response.")
    expect(alert).toHaveFocus()
    fireEvent.click(screen.getByRole("button", { name: "Retry Controller check" }))
    expect(await screen.findByRole("heading", { name: "Sign in to Controller" })).toBeVisible()
    expect(screen.getByLabelText("Controller password")).toHaveFocus()
  })

  it("keeps the authenticated shell operable when logout fails and allows retry", async () => {
    // Given: an authenticated shell whose first logout request fails.
    let logoutAttempts = 0
    const fetcher = vi.fn<typeof fetch>((input) => {
      switch (pathFor(input)) {
        case "/api/auth/setup": return Promise.resolve(response(initializedSetup))
        case "/api/auth/session": return Promise.resolve(response(session, 200, "proof"))
        case "/api/tasks": return Promise.resolve(response(emptyTaskPage))
        case "/api/status-counts": return Promise.resolve(response(emptyTaskCounts))
        case "/api/workers": return Promise.resolve(response(emptyWorkerList))
        case "/api/auth/logout": {
          logoutAttempts += 1
          return logoutAttempts === 1
            ? Promise.resolve(response({ error: { code: "unavailable", message: "remote worker is unavailable", retryable: true, field_errors: [] } }, 503))
            : Promise.resolve(response({ logged_out: true }))
        }
        default: return Promise.resolve(response({ error: "internal" }, 500))
      }
    })
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
    fireEvent.click(screen.getByRole("link", { name: "Workers" }))
    expect(await screen.findByRole("heading", { name: "Workers" })).toBeVisible()
    expect(alert).toHaveFocus()
    fireEvent.click(screen.getByRole("button", { name: "Sign out" }))
    expect(await screen.findByRole("heading", { name: "Sign in to Controller" })).toBeVisible()
  })

  it("reports the actual event-stream lifecycle", async () => {
    // Given: an authenticated shell with a controllable EventSource.
    vi.stubGlobal("EventSource", FakeEventSource)
    vi.stubGlobal("fetch", authenticatedFetcher())
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
