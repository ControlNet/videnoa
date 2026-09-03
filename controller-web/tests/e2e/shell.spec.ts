import { expect, type Page, type Route, test } from "@playwright/test"

import { installOperationalReadRoutes } from "./operations-fixtures"

const evidenceDir = "../.omo/evidence/videnoa-controller/task-19/playwright-report/screenshots/task-15"
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
  expect(alertBox?.y).toBeGreaterThanOrEqual((headerBox?.y ?? 0) + (headerBox?.height ?? Number.MAX_SAFE_INTEGER))
  await expect(page.getByRole("heading", { name: "Tasks" })).toBeVisible()
  await page.getByRole("button", { name: "Sign out" }).click()
  await expect(page.getByRole("heading", { name: "Sign in to Controller" })).toBeVisible()
})
