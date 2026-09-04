import { expect, type Page, type Route, test } from "@playwright/test"

import { installOperationalReadRoutes } from "./operations-fixtures"

const evidenceDir = "../.omo/evidence/videnoa-controller/task-19/playwright-report/screenshots/task-15"
const authFocusEvidence = "../.omo/evidence/videnoa-controller/final/remediation-auth-focus/malformed-login-focused.png"
const session = {
  id: "550e8400-e29b-41d4-a716-446655440000",
  authenticated: true,
  method: "session",
  expires_at: "2030-01-01T00:00:00Z",
  idle_expires_at: "2030-01-01T00:00:00Z",
} as const
const emptyTaskPage = { items: [], total: 0, limit: 50, offset: 0 }
const emptyTaskCounts = {
  items: ["queued", "reserved", "uploading", "staged", "submitting", "processing", "remote_completed", "downloading", "verifying", "publishing", "remote_cleanup", "completed", "failed", "cancelled"].map((status) => ({ status, count: 0 })),
  total: 0,
}

async function json(route: Route, body: unknown, status = 200, headers: Record<string, string> = {}): Promise<void> {
  await route.fulfill({ status, contentType: "application/json", headers, body: JSON.stringify(body) })
}

async function installApi(page: Page): Promise<{ readonly expire: () => void }> {
  let authenticated = false
  let expired = false
  await page.route("**/api/events", async (route) => {
    await route.fulfill({ status: 200, contentType: "text/event-stream", body: "event: refetch\ndata: {\"reason\":\"snapshot_required\"}\n\n" })
  })
  await page.route("**/api/auth/session", async (route) => {
    if (authenticated && !expired) await json(route, session, 200, { "x-csrf-token": "session-proof" })
    else await json(route, { error: "unauthorized" }, 401)
  })
  await page.route("**/api/auth/login", async (route) => {
    authenticated = true
    await json(route, { session }, 200, { "x-csrf-token": "login-proof" })
  })
  await page.route("**/api/auth/logout", async (route) => {
    authenticated = false
    await json(route, { logged_out: true })
  })
  await page.route("**/api/tasks?*", async (route) => json(route, emptyTaskPage))
  await page.route("**/api/status-counts", async (route) => json(route, emptyTaskCounts))
  await installOperationalReadRoutes(page)
  return { expire: () => { expired = true } }
}

async function signIn(page: Page): Promise<void> {
  await page.goto("/tasks")
  await page.getByLabel("Controller password").fill("transient-test-password")
  await page.getByRole("button", { name: "Sign in" }).click()
  await expect(page.getByRole("heading", { name: "Tasks" })).toBeVisible()
}

test("login, protected navigation, reload, narrow layout, and logout", async ({ page }) => {
  // Given: same-origin Controller auth routes with no initial session.
  await installApi(page)

  // When: the operator signs in and uses every shell route.
  await signIn(page)
  await page.screenshot({ path: `${evidenceDir}/shell-desktop.png`, fullPage: true })
  await page.getByRole("link", { name: "Workers" }).click()
  await expect(page).toHaveURL(/\/workers$/)
  await expect(page.getByText("render-east")).toBeVisible()
  await page.getByRole("link", { name: "Settings" }).click()
  await expect(page).toHaveURL(/\/settings$/)
  await expect(page.getByText("Runtime settings")).toBeVisible()
  await page.reload()
  await expect(page.getByRole("heading", { name: "Settings" })).toBeVisible()
  await expect(page.getByRole("main")).toBeFocused()
  await page.setViewportSize({ width: 375, height: 812 })
  await expect(page.locator(".connection-status")).toBeVisible()
  await expect(page.locator(".connection-status")).toContainText("Controller")
  await page.screenshot({ path: `${evidenceDir}/shell-narrow.png`, fullPage: true })

  // Then: navigation remains usable, storage contains no auth material, and logout protects routes.
  expect(await page.evaluate(() => ({ local: { ...localStorage }, session: { ...sessionStorage }, overflow: document.documentElement.scrollWidth > document.documentElement.clientWidth }))).toEqual({ local: {}, session: {}, overflow: false })
  await page.getByRole("button", { name: "Sign out" }).click()
  await expect(page.getByRole("heading", { name: "Sign in to Controller" })).toBeVisible()
  await page.goto("/workers")
  await expect(page.getByRole("heading", { name: "Sign in to Controller" })).toBeVisible()
})

test("desktop Settings wheel scroll reaches final content while Sign out stays visible", async ({ page }) => {
  // Given: an authenticated desktop shell with the complete runtime Settings surface.
  await page.setViewportSize({ width: 1440, height: 900 })
  await installApi(page)
  await signIn(page)
  await page.getByRole("link", { name: "Settings" }).click()
  await expect(page.getByRole("heading", { name: "Restart-required configuration" })).toBeVisible()
  await page.locator(".app-frame").evaluate(async (element) => {
    await Promise.all(element.getAnimations({ subtree: true }).map((animation) => animation.finished))
  })
  const main = page.locator(".shell-main")
  await main.evaluate((element) => { element.scrollTop = 0 })

  // When: the operator uses a normal wheel gesture over the route body.
  await main.hover()
  await page.mouse.wheel(0, 1_000)

  // Then: only main scrolls, the final Settings content is reachable, and Sign out remains fixed.
  await expect.poll(() => main.evaluate((element) => element.scrollTop)).toBeGreaterThan(0)
  await expect(page.getByRole("button", { name: "Save runtime settings" })).toBeInViewport({ ratio: 1 })
  await expect(page.locator(".read-only-settings .readiness-check").last()).toBeInViewport({ ratio: 1 })
  await expect(page.getByRole("button", { name: "Sign out" })).toBeInViewport({ ratio: 1 })
  expect(await page.evaluate(() => {
    const frame = document.querySelector(".app-frame")
    if (!(frame instanceof HTMLElement)) throw new TypeError("application frame is missing")
    return { documentScrollTop: document.documentElement.scrollTop, frameScrollTop: frame.scrollTop, frameClientHeight: frame.clientHeight, frameScrollHeight: frame.scrollHeight }
  })).toEqual({ documentScrollTop: 0, frameScrollTop: 0, frameClientHeight: 900, frameScrollHeight: 900 })
})

test("wrong password, malformed response, network failure, and expiry remain recoverable", async ({ page }) => {
  // Given: deterministic failure modes at the same-origin login boundary.
  let attempt = 0
  await page.route("**/api/auth/session", async (route) => json(route, { error: "unauthorized" }, 401))
  await page.route("**/api/auth/login", async (route) => {
    attempt += 1
    if (attempt === 1) await json(route, { error: "unauthorized" }, 401)
    else if (attempt === 2) await route.fulfill({ status: 200, contentType: "application/json", body: "{" })
    else await route.abort("failed")
  })
  await page.goto("/")

  // When/Then: each failure is explained and the form remains operable.
  for (const expected of ["The password was not accepted.", "Controller returned an invalid response.", "Controller could not be reached."]) {
    await page.getByLabel("Controller password").fill("never-recorded")
    await page.getByRole("button", { name: "Sign in" }).click()
    await expect(page.getByRole("alert")).toContainText(expected)
  }
})

test("malformed login response focuses its summary and supports keyboard recovery", async ({ page }) => {
  // Given: an unauthenticated login whose first response is malformed and whose retry succeeds.
  await page.setViewportSize({ width: 1280, height: 900 })
  let loginAttempts = 0
  await page.route("**/api/events", async (route) => {
    await route.fulfill({ status: 200, contentType: "text/event-stream", body: "" })
  })
  await page.route("**/api/auth/session", async (route) => json(route, { error: "unauthorized" }, 401))
  await page.route("**/api/auth/login", async (route) => {
    loginAttempts += 1
    if (loginAttempts === 1) {
      await route.fulfill({ status: 200, contentType: "application/json", body: "{" })
      return
    }
    await json(route, { session }, 200, { "x-csrf-token": "login-proof" })
  })
  await page.route("**/api/tasks?*", async (route) => json(route, emptyTaskPage))
  await page.route("**/api/status-counts", async (route) => json(route, emptyTaskCounts))
  await installOperationalReadRoutes(page)
  await page.goto("/")
  const password = page.getByLabel("Controller password")
  await password.fill("synthetic-keyboard-value")

  // When: the operator submits and receives the recoverable malformed-response summary.
  await password.press("Enter")

  // Then: the summary owns visible focus, Tab returns to the field, and Enter retries successfully.
  const alert = page.getByRole("alert")
  await expect(alert).toContainText("Controller returned an invalid response.")
  await expect(alert).toBeFocused()
  expect(await alert.evaluate((element) => getComputedStyle(element).outlineStyle)).not.toBe("none")
  await page.screenshot({ path: authFocusEvidence, fullPage: false })
  await page.keyboard.press("Tab")
  await expect(password).toBeFocused()
  await password.press("Enter")
  await expect(page.getByRole("heading", { name: "Tasks" })).toBeVisible()
  await expect(page.getByRole("main")).toBeFocused()
})

test("session expiry redirects to login", async ({ page }) => {
  // Given: an authenticated session that later expires server-side.
  const api = await installApi(page)
  await signIn(page)

  // When: a reload checks the now-expired session.
  api.expire()
  await page.reload()

  // Then: the protected shell clears and login receives focus.
  await expect(page.getByRole("heading", { name: "Sign in to Controller" })).toBeVisible()
  await expect(page.getByLabel("Controller password")).toBeFocused()
})

test("logout failure keeps the shell authenticated and permits retry", async ({ page }) => {
  // Given: an authenticated shell whose first logout request is unavailable.
  let logoutAttempts = 0
  await page.route("**/api/events", async (route) => {
    await route.fulfill({ status: 200, contentType: "text/event-stream", body: "event: refetch\ndata: {\"reason\":\"snapshot_required\"}\n\n" })
  })
  await page.route("**/api/auth/session", async (route) => json(route, session, 200, { "x-csrf-token": "session-proof" }))
  await page.route("**/api/tasks?*", async (route) => json(route, emptyTaskPage))
  await page.route("**/api/status-counts", async (route) => json(route, emptyTaskCounts))
  await page.route("**/api/auth/logout", async (route) => {
    logoutAttempts += 1
    if (logoutAttempts === 1) {
      await json(route, { error: { code: "unavailable", message: "remote worker is unavailable", retryable: true, field_errors: [] } }, 503)
      return
    }
    await json(route, { logged_out: true })
  })
  await page.goto("/tasks")
  await expect(page.getByRole("heading", { name: "Tasks" })).toBeVisible()
  await page.setViewportSize({ width: 375, height: 812 })

  // When: the operator retries after the recoverable failure.
  await page.getByRole("button", { name: "Sign out" }).click()

  // Then: focus reaches the alert, the shell remains, and the second attempt signs out.
  const alert = page.getByRole("alert")
  await expect(alert).toContainText("Controller could not complete sign out. Try again.")
  await expect(alert).toBeFocused()
  const alertBox = await alert.boundingBox()
  const headerBox = await page.locator(".shell-sidebar").boundingBox()
  if (alertBox === null || headerBox === null) throw new TypeError("logout alert shell geometry is missing")
  expect(alertBox.y).toBeGreaterThanOrEqual(headerBox.y + headerBox.height)
  expect(await alert.evaluate((element) => {
    const box = element.getBoundingClientRect()
    const style = getComputedStyle(element)
    const focusExtent = Number.parseFloat(style.outlineWidth) + Number.parseFloat(style.outlineOffset)
    return box.left - focusExtent >= 0 && box.top - focusExtent >= 0 && box.right + focusExtent <= innerWidth && box.bottom + focusExtent <= innerHeight
  })).toBe(true)
  expect(await page.locator(".shell-main").evaluate((main) => {
    const alertElement = document.querySelector(".shell-alert")
    if (!(alertElement instanceof HTMLElement)) throw new TypeError("logout alert is missing")
    const alertBounds = alertElement.getBoundingClientRect()
    return Array.from(main.querySelectorAll<HTMLElement>("button:not(:disabled), input:not(:disabled), select:not(:disabled), a[href]"))
      .filter((control) => {
        const bounds = control.getBoundingClientRect()
        const visible = bounds.right > 0 && bounds.bottom > 0 && bounds.left < innerWidth && bounds.top < innerHeight
        return visible && bounds.left < alertBounds.right && bounds.right > alertBounds.left && bounds.top < alertBounds.bottom && bounds.bottom > alertBounds.top
      })
      .map((control) => control.getAttribute("aria-label") ?? control.textContent?.trim() ?? control.tagName)
  })).toEqual([])
  await expect(page.getByRole("heading", { name: "Tasks" })).toBeVisible()
  await page.getByRole("button", { name: "Sign out" }).click()
  await expect(page.getByRole("heading", { name: "Sign in to Controller" })).toBeVisible()
})
