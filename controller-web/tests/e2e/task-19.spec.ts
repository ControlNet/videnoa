import { mkdir } from "node:fs/promises"

import AxeBuilder from "@axe-core/playwright"
import { expect, type Page, test } from "@playwright/test"

import { installOperationalApi } from "./operations-fixtures"
import { fulfillJson, statuses, task } from "./tasks-fixtures"

const failureEvidenceDir = "../.omo/evidence/videnoa-controller/task-19/visual-failures"
const session = {
  id: "550e8400-e29b-41d4-a716-446655440000",
  authenticated: true,
  method: "session",
  expires_at: "2030-01-01T00:00:00Z",
  idle_expires_at: "2030-01-01T00:00:00Z",
} as const

test.beforeEach(async () => mkdir(failureEvidenceDir, { recursive: true }))

test("has no serious accessibility violations on login and operational routes", async ({ page }) => {
  await disableEventSource(page)
  await page.route("**/api/auth/session", async (route) => fulfillJson(route, { error: "unauthorized" }, 401))
  await page.goto("/")
  await expect(page.getByRole("heading", { name: "Sign in to Controller" })).toBeVisible()
  await expectNoSeriousViolations(page)

  await page.unrouteAll({ behavior: "wait" })
  await installOperationalApi(page)
  await installTaskRoutes(page, false)
  for (const path of ["/tasks?columns=path,error", "/workers", "/settings"] as const) {
    await page.goto(path)
    await expect(page.locator("h1")).toBeVisible()
    await expectNoSeriousViolations(page)
  }
})

test("provides keyboard-operable worker table overflow controls", async ({ page }) => {
  await installOperationalApi(page)
  await page.setViewportSize({ width: 375, height: 812 })
  await page.goto("/workers")
  const frame = page.getByRole("region", { name: "Scrollable worker results" })
  await expect(frame).toHaveAttribute("aria-describedby", "worker-table-scroll-hint")
  await page.getByRole("button", { name: "Scroll worker table right" }).click()
  await expect.poll(() => frame.evaluate((element) => element.scrollLeft)).toBeGreaterThan(0)
})

test("restores focus to the worker edit trigger and contains the narrow dialog", async ({ page }) => {
  await installOperationalApi(page)
  await page.setViewportSize({ width: 375, height: 420 })
  await page.goto("/workers")
  const editTrigger = page.getByRole("button", { name: "Edit render-east" })
  await editTrigger.click()
  const dialog = page.getByRole("dialog", { name: "Edit Worker" })
  await expect(dialog).toBeVisible()
  expect(await dialog.evaluate((element) => {
    const bounds = element.getBoundingClientRect()
    return {
      contained: bounds.top >= 0 && bounds.bottom <= window.innerHeight,
      overflowY: getComputedStyle(element).overflowY,
    }
  })).toEqual({ contained: true, overflowY: "auto" })
  await page.getByRole("button", { name: "Close worker form" }).click()
  await expect(editTrigger).toBeFocused()
})

test("announces SSE reconnect and unavailable states while refetching bounded data", async ({ page }) => {
  await installEventSourceHarness(page)
  const taskReads = await installTaskRoutes(page, false)
  await page.goto("/tasks")
  await expect(page.getByRole("table")).toBeVisible()
  await dispatchConnectionEvent(page, "open", 1)
  await expect(page.getByText("Controller connected", { exact: true })).toBeVisible()
  const readsBeforeReconnect = taskReads.count()
  await dispatchConnectionEvent(page, "error", 0)
  await expect(page.getByText("Controller reconnecting", { exact: true })).toBeVisible()
  await expect.poll(taskReads.count).toBeGreaterThan(readsBeforeReconnect)
  await page.screenshot({ path: `${failureEvidenceDir}/sse-reconnecting.png`, animations: "disabled", fullPage: false, scale: "css" })
  await dispatchConnectionEvent(page, "error", 2)
  await expect(page.getByText("Controller unavailable", { exact: true })).toBeVisible()
  await page.screenshot({ path: `${failureEvidenceDir}/sse-unavailable.png`, animations: "disabled", fullPage: false, scale: "css" })
})

test("recovers an API 500 and contains long CJK task evidence at narrow width", async ({ page }) => {
  await disableEventSource(page)
  await installTaskRoutes(page, true)
  await page.setViewportSize({ width: 375, height: 812 })
  await page.emulateMedia({ colorScheme: "dark", reducedMotion: "reduce" })
  await page.goto("/tasks?columns=path,error")
  const loadError = page.getByRole("alert")
  await expect(loadError).toContainText("Controller could not load task history.")
  await loadError.scrollIntoViewIfNeeded()
  await page.screenshot({ path: `${failureEvidenceDir}/api-500-narrow.png`, animations: "disabled", fullPage: false, scale: "css" })
  await page.getByRole("button", { name: "Retry" }).click()
  await expect(page.getByRole("button", { name: "Open task 00000000-0000-4000-8000-000000000013" })).toBeVisible()
  expect(await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth)).toBe(false)
  await expect(page.locator("td.long-cell").first()).toHaveCSS("text-overflow", "ellipsis")
  await page.screenshot({ path: `${failureEvidenceDir}/cjk-long-path-narrow.png`, animations: "disabled", fullPage: false, scale: "css" })
})

test("preserves visible focus and instant feedback in forced colors and reduced motion", async ({ page }) => {
  await installOperationalApi(page)
  await page.setViewportSize({ width: 768, height: 900 })
  await page.emulateMedia({ forcedColors: "active", reducedMotion: "reduce" })
  await page.goto("/workers")
  await page.getByRole("link", { name: "Workers" }).focus()
  const styles = await page.getByRole("link", { name: "Workers" }).evaluate((element) => {
    const computed = getComputedStyle(element)
    return { animationDuration: computed.animationDuration, outlineStyle: computed.outlineStyle, transitionDuration: computed.transitionDuration }
  })
  expect(Number.parseFloat(styles.animationDuration)).toBeLessThanOrEqual(0.00001)
  expect(Number.parseFloat(styles.transitionDuration)).toBeLessThanOrEqual(0.00001)
  expect(styles.outlineStyle).not.toBe("none")
  await expect(page.getByText("Offline", { exact: true })).toBeVisible()
  await expect(page.getByText("Enabled", { exact: true })).toBeVisible()
  await page.screenshot({ path: `${failureEvidenceDir}/forced-colors-reduced-motion.png`, animations: "disabled", fullPage: false, scale: "css" })
})

test("expires to a clean login surface without browser-stored credentials", async ({ page, context }) => {
  await disableEventSource(page)
  await context.addCookies([{
    name: "videnoa_session",
    value: crypto.randomUUID(),
    url: "http://127.0.0.1:4173",
    httpOnly: true,
    sameSite: "Strict",
  }])
  expect((await context.cookies()).map((cookie) => cookie.name)).toContain("videnoa_session")
  let authenticated = true
  await page.route("**/api/auth/session", async (route) => {
    if (authenticated) {
      await fulfillJson(route, session)
      return
    }
    await route.fulfill({
      status: 401,
      contentType: "application/json",
      headers: { "set-cookie": "videnoa_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0" },
      body: JSON.stringify({ error: "unauthorized" }),
    })
  })
  await installTaskRoutes(page, false, false)
  await page.setViewportSize({ width: 375, height: 812 })
  await page.goto("/tasks")
  await expect(page.getByRole("heading", { name: "Tasks" })).toBeVisible()
  authenticated = false
  await page.reload()
  await expect(page.getByRole("heading", { name: "Sign in to Controller" })).toBeVisible()
  await expect(page.getByLabel("Controller password")).toBeFocused()
  expect(await page.evaluate(async () => ({
    cacheKeys: await caches.keys(),
    indexedDatabases: await indexedDB.databases(),
    local: Object.keys(localStorage),
    session: Object.keys(sessionStorage),
  }))).toEqual({ cacheKeys: [], indexedDatabases: [], local: [], session: [] })
  expect((await context.cookies()).map((cookie) => cookie.name)).not.toContain("videnoa_session")
  await page.screenshot({ path: `${failureEvidenceDir}/session-expired-narrow.png`, animations: "disabled", fullPage: false, scale: "css" })
})

async function expectNoSeriousViolations(page: Page): Promise<void> {
  const results = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"]).analyze()
  expect(results.violations.filter((violation) => violation.impact === "serious" || violation.impact === "critical")).toEqual([])
}

async function disableEventSource(page: Page): Promise<void> {
  await page.addInitScript(() => Object.defineProperty(window, "EventSource", { configurable: true, value: undefined }))
}

async function installTaskRoutes(page: Page, failFirst: boolean, installSession = true): Promise<{ readonly count: () => number }> {
  let reads = 0
  if (installSession) await page.route("**/api/auth/session", async (route) => fulfillJson(route, session))
  await page.route("**/api/status-counts", async (route) => fulfillJson(route, {
    items: statuses.map((status) => ({ status, count: status === "failed" ? 1 : 0 })),
    total: 1,
  }))
  await page.route("**/api/tasks?*", async (route) => {
    reads += 1
    if (failFirst && reads === 1) {
      await fulfillJson(route, { error: { code: "internal_error", message: "internal failure", retryable: true, field_errors: [] } }, 500)
      return
    }
    const cjkTask = task(12, {
      status: "failed",
      input_path: "/媒体/アニメ/最終章/非常に長い保存場所/劇場版-最終章-超長編成名-第十二話.mkv",
      output_path: "/輸出/完成作品/劇場版-最終章-超長編成名-第十二話.mp4",
      failure: { failure_stage: "processing", failure_code: "processing_failed", message: "處理節點回報長篇診斷：模型載入失敗，但完整訊息必須保持在表格邊界內。", retryable: true },
    })
    await fulfillJson(route, { items: [cjkTask], total: 1, limit: 50, offset: 0 })
  })
  return { count: () => reads }
}

async function installEventSourceHarness(page: Page): Promise<void> {
  await page.addInitScript(() => {
    class Task19EventSource extends EventTarget {
      static readonly CONNECTING = 0
      static readonly OPEN = 1
      static readonly CLOSED = 2
      readyState = Task19EventSource.CONNECTING
      constructor() {
        super()
        Reflect.set(window, "task19EventSource", this)
      }
      close(): void { this.readyState = Task19EventSource.CLOSED }
    }
    Object.defineProperty(window, "EventSource", { configurable: true, value: Task19EventSource })
  })
}

async function dispatchConnectionEvent(page: Page, type: "open" | "error", readyState: number): Promise<void> {
  await page.evaluate(({ eventType, state }) => {
    const source = Reflect.get(window, "task19EventSource")
    if (!(source instanceof EventTarget)) throw new TypeError("Task 19 EventSource was not initialized")
    Reflect.set(source, "readyState", state)
    source.dispatchEvent(new Event(eventType))
  }, { eventType: type, state: readyState })
}
