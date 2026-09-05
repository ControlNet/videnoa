import { expect, test } from "@playwright/test"

import {
  appendEvidence,
  capture,
  fulfillJson,
  installAuthenticatedSession,
  installPagedApi,
  requestJournal,
  statuses,
  task,
} from "./tasks-fixtures"

const emptyCounts = { items: statuses.map((status) => ({ status, count: 0 })), total: 0 }

test("suppresses prior rows when a changed query fails", async ({ page }) => {
  // Given: a successful initial page and a failing changed-query request.
  await page.addInitScript(() => Object.defineProperty(window, "EventSource", { value: undefined }))
  await installAuthenticatedSession(page)
  await page.route("**/api/status-counts", async (route) => fulfillJson(route, emptyCounts))
  await page.route("**/api/tasks?*", async (route) => {
    const url = new URL(route.request().url())
    if (url.searchParams.get("search") === "new-query") {
      await fulfillJson(route, { error: { code: "unavailable", message: "busy", retryable: true, field_errors: [] } }, 503)
      return
    }
    await fulfillJson(route, { items: [task(0)], total: 1, limit: 50, offset: 0 })
  })
  await page.goto("/tasks")
  await expect(page.getByRole("table").locator("tbody tr")).toHaveCount(1)

  // When: the URL-bound search changes to the failing request generation.
  await page.getByLabel("Search task paths").fill("new-query")
  await expect(page).toHaveURL(/search=new-query/)
  await expect(page.getByRole("alert")).toContainText("Controller could not load task history.")

  // Then: no prior-query row remains under the current URL.
  await expect(page.getByText("episode-00001.mkv")).toHaveCount(0)
  await expect(page.getByRole("table")).toHaveCount(0)
  await appendEvidence("changed-query failure: prior page rows suppressed; recoverable alert visible under search=new-query")
})

test("shows bounded loading state until a slow page resolves", async ({ page }) => {
  // Given: counts resolve while the task page is held by a deterministic gate.
  let releaseRequest = (): void => undefined
  const gate = new Promise<void>((resolve) => { releaseRequest = resolve })
  await page.addInitScript(() => Object.defineProperty(window, "EventSource", { value: undefined }))
  await installAuthenticatedSession(page)
  await page.route("**/api/status-counts", async (route) => fulfillJson(route, emptyCounts))
  await page.route("**/api/tasks?*", async (route) => {
    await gate
    await fulfillJson(route, { items: [task(0)], total: 1, limit: 50, offset: 0 })
  })

  // When: the page loads before the held request is released.
  await page.goto("/tasks")
  await expect(page.locator(".task-table-frame")).toHaveAttribute("aria-busy", "true")
  await expect(page.locator(".loading-row")).toHaveCount(8)
  releaseRequest()

  // Then: the bounded table converges to the successful row.
  await expect(page.getByRole("table").locator("tbody tr")).toHaveCount(1)
  await appendEvidence("slow call: aria-busy table showed 8 bounded loading rows, then resolved to 1 task row")
})

test("recovers a failed call through Retry", async ({ page }) => {
  // Given: the first bounded task request fails and the second succeeds.
  let taskRequests = 0
  await page.addInitScript(() => Object.defineProperty(window, "EventSource", { value: undefined }))
  await installAuthenticatedSession(page)
  await page.route("**/api/status-counts", async (route) => fulfillJson(route, emptyCounts))
  await page.route("**/api/tasks?*", async (route) => {
    taskRequests += 1
    if (taskRequests === 1) {
      await fulfillJson(route, { error: { code: "unavailable", message: "database busy", retryable: true, field_errors: [] } }, 503)
      return
    }
    await fulfillJson(route, { items: [task(0)], total: 1, limit: 50, offset: 0 })
  })
  await page.goto("/tasks")
  await expect(page.getByRole("alert")).toContainText("Controller could not load task history.")

  // When: the operator retries the same bounded query.
  await page.getByRole("button", { name: "Retry" }).click()

  // Then: one successful retry replaces the alert with current rows.
  await expect(page.getByRole("table").locator("tbody tr")).toHaveCount(1)
  await expect(page.getByRole("alert")).toHaveCount(0)
  expect(taskRequests).toBe(2)
  await appendEvidence("failed call then Retry: initial 503 alert recovered on request 2 with current task rows")
})

test("falls back from invalid URL query values without weakening request bounds", async ({ page }) => {
  // Given: a fully deterministic bounded server.
  const journal = requestJournal()
  await installPagedApi(page, journal, 100)

  // When: unsupported URL values reach the query boundary.
  await page.goto("/tasks?status=bogus&source=bogus&failure_stage=bogus&sort=nope&order=sideways&limit=999&offset=-4&columns=input_path,bogus,error")
  await expect(page.getByRole("table").locator("tbody tr")).toHaveCount(50)

  // Then: the API receives only canonical fallback values and supported columns render.
  const request = new URLSearchParams(journal.tasks[0])
  expect(Object.fromEntries(request)).toEqual({ limit: "50", offset: "0", sort: "priority", direction: "desc" })
  await expect(page.getByRole("columnheader", { name: "Input Path", exact: true })).toBeVisible()
  await expect(page.getByRole("columnheader", { name: "error" })).toBeVisible()
  await appendEvidence("invalid URL fallback: unsupported status/sort/order/limit/offset omitted or reset; API limit=50 offset=0")
})

test("contains long paths and errors inside table-owned overflow", async ({ page }) => {
  // Given: failed rows with long path and diagnostic columns enabled.
  const journal = requestJournal()
  await installPagedApi(page, journal)
  await page.setViewportSize({ width: 375, height: 812 })
  await page.emulateMedia({ colorScheme: "dark", reducedMotion: "reduce" })
  await page.goto("/tasks?status=failed&columns=input_path,error")
  await expect(page.getByRole("table").locator("tbody tr")).toHaveCount(50)

  // When: a keyboard user activates the visible control for clipped columns.
  const frame = page.getByRole("region", { name: "Scrollable task results" })
  await page.getByRole("button", { name: "Scroll task table right" }).click()
  await expect.poll(() => frame.evaluate((element) => element.scrollLeft)).toBeGreaterThan(0)

  // When: the narrow table is scrolled to its long-content columns.
  const errorHeader = page.getByRole("columnheader", { name: "error" })
  await errorHeader.evaluate((element) => {
    const scrollFrame = element.closest(".task-table-frame")
    if (!(scrollFrame instanceof HTMLElement)) return
    const headerBounds = element.getBoundingClientRect()
    const frameBounds = scrollFrame.getBoundingClientRect()
    const target = scrollFrame.scrollLeft + headerBounds.left - frameBounds.left
    scrollFrame.scrollLeft = Math.max(0, Math.min(scrollFrame.scrollWidth - scrollFrame.clientWidth, target))
  })
  await frame.evaluate((element) => element.scrollIntoView({ block: "center" }))

  // Then: only the table owns overflow and long cells retain their visible header context.
  expect(await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth)).toBe(false)
  expect(await frame.evaluate((element) => element.scrollWidth > element.clientWidth)).toBe(true)
  await expect.poll(() => errorHeader.evaluate((element) => {
    const scrollFrame = element.closest(".task-table-frame")
    if (!(scrollFrame instanceof HTMLElement)) return false
    const headerBounds = element.getBoundingClientRect()
    const frameBounds = scrollFrame.getBoundingClientRect()
    const contentStart = headerBounds.left + Number.parseFloat(getComputedStyle(element).paddingInlineStart)
    return headerBounds.left >= frameBounds.left && contentStart < frameBounds.right
  })).toBe(true)
  await expect(page.locator("td.long-cell").first()).toHaveCSS("text-overflow", "ellipsis")
  await capture(page, "long-content-narrow.png")
  await appendEvidence("long paths/errors: document overflow=false; table overflow=true; long cells use ellipsis containment")
})

test("does not correct contradictory empty-page metadata or loop", async ({ page }) => {
  // Given: a server contradicts the requested deep offset in its empty response metadata.
  const journal = requestJournal()
  await page.addInitScript(() => Object.defineProperty(window, "EventSource", { value: undefined }))
  await installAuthenticatedSession(page)
  await page.route("**/api/status-counts", async (route) => {
    journal.counts.push(route.request().url())
    await fulfillJson(route, emptyCounts)
  })
  await page.route("**/api/tasks?*", async (route) => {
    journal.tasks.push(new URL(route.request().url()).search)
    await fulfillJson(route, { items: [], total: 123, limit: 50, offset: 0 })
  })

  // When: the contradictory page is rendered.
  await page.goto("/tasks?offset=10000")
  await expect(page.getByText("No tasks match this view.")).toBeVisible()

  // Then: client correction refuses untrusted metadata and issues no loop.
  await expect.poll(() => journal.tasks.length).toBe(1)
  expect(journal.counts).toHaveLength(1)
  await expect(page.locator(".task-pagination")).toContainText("0-0 of 123")
  await appendEvidence("contradictory empty metadata: response offset mismatch prevented correction loop; task requests=1; count requests=1")
})
