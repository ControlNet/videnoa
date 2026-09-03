import { expect, type Page, type Route, test } from "@playwright/test"

const evidenceDir = "../.omo/evidence/videnoa-controller/task-15/visual-qa"
const viewports = {
  narrow: { width: 375, height: 812 },
  tablet: { width: 768, height: 900 },
  desktop: { width: 1280, height: 900 },
} as const

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

async function installApi(page: Page): Promise<void> {
  let authenticated = false
  await page.route("**/api/events", async (route) => {
    await route.fulfill({ status: 200, contentType: "text/event-stream", body: "event: refetch\ndata: {\"reason\":\"snapshot_required\"}\n\n" })
  })
  await page.route("**/api/auth/session", async (route) => {
    if (authenticated) await json(route, session, 200, { "x-csrf-token": "session-proof" })
    else await json(route, { error: "unauthorized" }, 401)
  })
  await page.route("**/api/auth/login", async (route) => {
    authenticated = true
    await json(route, { session }, 200, { "x-csrf-token": "login-proof" })
  })
  await page.route("**/api/auth/logout", async (route) => {
    await json(route, { error: { code: "unavailable", message: "remote worker is unavailable", retryable: true, field_errors: [] } }, 503)
  })
  await page.route("**/api/tasks?*", async (route) => json(route, emptyTaskPage))
  await page.route("**/api/status-counts", async (route) => json(route, emptyTaskCounts))
}

async function settleAndCapture(page: Page, name: string, viewport: { readonly width: number; readonly height: number }): Promise<void> {
  await page.setViewportSize(viewport)
  await page.emulateMedia({ colorScheme: "dark", reducedMotion: "reduce" })
  await page.evaluate(async () => {
    await Promise.all(document.getAnimations().map(async (animation) => animation.finished.catch(() => undefined)))
  })
  expect(await page.evaluate(() => ({
    width: document.documentElement.clientWidth,
    height: document.documentElement.clientHeight,
    overflow: document.documentElement.scrollWidth > document.documentElement.clientWidth,
  }))).toEqual({ ...viewport, overflow: false })
  await page.screenshot({
    path: `${evidenceDir}/${name}.png`,
    animations: "disabled",
    fullPage: false,
    scale: "css",
  })
}

async function expectNoRenderedGradients(page: Page): Promise<void> {
  const backgroundImages = await page.evaluate(() => {
    const selectors = [".login-page", ".boundary-line", ".readiness-panel"]
    const values = selectors.flatMap((selector) => {
      const element = document.querySelector(selector)
      if (element === null) return []
      return [getComputedStyle(element).backgroundImage]
    })
    const loginPage = document.querySelector(".login-page")
    if (loginPage !== null) values.push(getComputedStyle(loginPage, "::after").backgroundImage)
    return values
  })
  expect(backgroundImages.every((backgroundImage) => !backgroundImage.includes("gradient"))).toBe(true)
}

test("captures the complete Task 15 visual evidence matrix deterministically", async ({ page }) => {
  // Given: deterministic same-origin auth routes and reduced-motion dark rendering.
  await installApi(page)
  await page.goto("/")

  // When: every required login viewport is captured before authentication.
  await expect(page.getByRole("heading", { name: "Sign in to Controller" })).toBeVisible()
  await expectNoRenderedGradients(page)
  for (const [viewportName, viewport] of Object.entries(viewports)) {
    await settleAndCapture(page, `login-${viewportName}`, viewport)
  }

  // When: the operator signs in and visits each protected route at every viewport.
  await page.getByLabel("Controller password").fill("transient-test-password")
  await page.getByRole("button", { name: "Sign in" }).click()
  for (const route of ["tasks", "workers", "settings"] as const) {
    await page.getByRole("link", { name: `${route[0]?.toUpperCase()}${route.slice(1)}` }).click()
    await expect(page).toHaveURL(new RegExp(`/${route}$`))
    await expectNoRenderedGradients(page)
    if (route !== "tasks") {
      const readinessType = await page.evaluate(() => ({
        token: getComputedStyle(document.documentElement).getPropertyValue("--type-subtitle").trim(),
        root: getComputedStyle(document.documentElement).fontSize,
        heading: (() => {
          const heading = document.querySelector(".readiness-panel h2")
          return heading === null ? "" : getComputedStyle(heading).fontSize
        })(),
      }))
      expect(Number.parseFloat(readinessType.heading)).toBe(Number.parseFloat(readinessType.token) * Number.parseFloat(readinessType.root))
    }
    for (const [viewportName, viewport] of Object.entries(viewports)) {
      await settleAndCapture(page, `${route}-${viewportName}`, viewport)
    }
  }

  // Then: failed sign-out evidence visibly records the alert as the focused element.
  await page.setViewportSize(viewports.narrow)
  await page.getByRole("button", { name: "Sign out" }).click()
  const alert = page.getByRole("alert")
  await expect(alert).toBeFocused()
  await expect(alert).toHaveCSS("outline-width", "3px")
  await settleAndCapture(page, "settings-logout-error-narrow", viewports.narrow)
  await settleAndCapture(page, "settings-logout-error-desktop", viewports.desktop)
})
