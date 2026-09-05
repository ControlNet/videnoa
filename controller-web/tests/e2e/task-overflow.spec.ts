import { mkdir, rm } from "node:fs/promises"

import AxeBuilder from "@axe-core/playwright"
import { expect, type Locator, type Page, test } from "@playwright/test"

import { expectUnavailableControlStyle } from "./control-style-assertions"
import { fulfillJson, statuses, task } from "./tasks-fixtures"

const evidenceDir = "../.omo/evidence/videnoa-controller/final/remediation-task-overflow"
const session = {
  id: "550e8400-e29b-41d4-a716-446655440000",
  authenticated: true,
  method: "session",
  expires_at: "2030-01-01T00:00:00Z",
  idle_expires_at: "2030-01-01T00:00:00Z",
} as const

test.beforeEach(async ({ page }) => {
  await rm(evidenceDir, { recursive: true, force: true })
  await mkdir(evidenceDir, { recursive: true })
  await installOverflowApi(page)
  await page.emulateMedia({ colorScheme: "dark", reducedMotion: "reduce" })
})

test("characterizes the native disabled transition at the right scroll edge", async ({ page }) => {
  // Given: an overflowing task table at its initial left boundary.
  await page.setViewportSize({ width: 1440, height: 900 })
  await page.goto("/tasks?columns=input_path,output_path,error,remote_job_id,attempts,duration,failure_stage,failure")
  const frame = page.getByRole("region", { name: "Scrollable task results" })
  const left = page.getByRole("button", { name: "Scroll task table left" })
  const right = page.getByRole("button", { name: "Scroll task table right" })
  await expect(left).toBeDisabled()
  await expect(right).toBeEnabled()

  // When: keyboard navigation moves directly to the effective right edge.
  await frame.focus()
  await frame.press("End")

  // Then: the native disabled state transfers to the right control.
  await expect(left).toBeEnabled()
  await expect(right).toBeDisabled()
})

test("keeps unavailable styling when right-edge layout geometry grows", async ({ page }) => {
  // Given: keyboard navigation has committed the unavailable right-edge state.
  await page.setViewportSize({ width: 1440, height: 900 })
  await page.goto("/tasks?columns=input_path,output_path,error,remote_job_id,attempts,duration,failure_stage,failure")
  const frame = page.getByRole("region", { name: "Scrollable task results" })
  const table = page.getByRole("table")
  const left = page.getByRole("button", { name: "Scroll task table left" })
  const right = page.getByRole("button", { name: "Scroll task table right" })
  await frame.focus()
  await frame.press("End")
  await expectUnavailableControlStyle(right, left)
  const initialPosition = await frame.evaluate((element) => element.scrollLeft)
  const initialMaximum = await frame.evaluate((element) => element.scrollWidth - element.clientWidth)

  // When: a settled layout change expands the table after the edge was reached.
  await table.evaluate((element) => {
    if (!(element instanceof HTMLTableElement)) throw new TypeError("task table is missing")
    element.style.minInlineSize = `${element.offsetWidth + 20}px`
  })
  await expect.poll(() => frame.evaluate((element) => element.scrollWidth - element.clientWidth)).toBeGreaterThan(initialMaximum)

  // Then: the frame stays anchored and the native and computed unavailable states agree.
  await expect.poll(() => frame.evaluate((element, previousPosition) => {
    const anchoredPosition = element.scrollLeft
    element.scrollLeft = Number.MAX_SAFE_INTEGER
    const saturatedPosition = element.scrollLeft
    return { anchored: anchoredPosition === saturatedPosition, grew: saturatedPosition > previousPosition }
  }, initialPosition)).toEqual({ anchored: true, grew: true })
  await expectUnavailableControlStyle(right, left)
})

test("keeps measured task overflow continuously discoverable and keyboard operable", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 })
  await page.goto("/tasks?columns=input_path,output_path,error,remote_job_id,attempts,duration,failure_stage,failure")
  const frame = page.getByRole("region", { name: "Scrollable task results" })
  const controls = page.getByRole("navigation", { name: "Task table horizontal navigation" })
  const left = page.getByRole("button", { name: "Scroll task table left" })
  const right = page.getByRole("button", { name: "Scroll task table right" })

  await expect(frame).toHaveAttribute("tabindex", "0")
  await expect(frame).toHaveAttribute("aria-describedby", "task-table-scroll-hint")
  await expect(controls).toBeVisible()
  await expect(left).toBeDisabled()
  await expect(right).toBeEnabled()
  await expectUnavailableControlStyle(left, right)
  await expectContained(page, frame)
  await expect(page.locator(".task-pagination")).toBeInViewport()
  await capture(page, "1440-left.png")

  await frame.focus()
  await expect(frame).toBeFocused()
  await expect(frame).toHaveCSS("outline-style", "solid")
  await frame.press("ArrowRight")
  await expect.poll(() => frame.evaluate((element) => element.scrollLeft)).toBeGreaterThan(0)
  await frame.press("End")
  await expect(right).toBeDisabled()
  await expect(left).toBeEnabled()
  await expectUnavailableControlStyle(right, left)
  await capture(page, "1440-right.png")

  await page.locator(".task-table").evaluate((element) => {
    if (!(element instanceof HTMLTableElement)) throw new TypeError("task table is missing")
    element.style.minInlineSize = "0"
    element.style.tableLayout = "fixed"
  })
  await expect(controls).toHaveCount(0)
  await expect(frame).toHaveAttribute("tabindex", "-1")
  await expect(frame).not.toHaveAttribute("aria-describedby")

  await page.locator(".task-table").evaluate((element) => {
    if (!(element instanceof HTMLTableElement)) throw new TypeError("task table is missing")
    element.style.removeProperty("min-inline-size")
    element.style.removeProperty("table-layout")
  })
  await page.setViewportSize({ width: 1024, height: 900 })
  await expect(controls).toBeVisible()
  await expect(right).toBeEnabled()
  await capture(page, "1024-left.png")
  await right.click()
  await expect.poll(() => frame.evaluate((element) => element.scrollLeft)).toBeGreaterThan(0)
  await capture(page, "1024-right.png")

  await page.setViewportSize({ width: 375, height: 812 })
  await frame.evaluate((element) => { element.scrollLeft = 0; element.scrollTop = 0 })
  await frame.scrollIntoViewIfNeeded()
  await expect(controls).toBeInViewport()
  await expect(page.locator(".task-pagination")).toBeInViewport()
  await expect(left).toBeDisabled()
  await capture(page, "375-cjk-left.png")

  await alignHeader(frame, page.getByRole("columnheader", { name: "error" }))
  await expect(page.locator("td.long-cell").first()).toHaveCSS("text-overflow", "ellipsis")
  await expect(page.getByText("處理節點回報長篇診斷", { exact: false }).first()).toBeAttached()
  await expectContained(page, frame)
  await capture(page, "375-cjk-right.png")

  const axe = await new AxeBuilder({ page }).include(".tasks-page").withTags(["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"]).analyze()
  expect(axe.violations.filter((violation) => violation.impact === "serious" || violation.impact === "critical")).toEqual([])
})

async function installOverflowApi(page: Page): Promise<void> {
  await page.addInitScript(() => Object.defineProperty(window, "EventSource", { configurable: true, value: undefined }))
  await page.route("**/api/auth/session", async (route) => fulfillJson(route, session))
  await page.route("**/api/status-counts", async (route) => fulfillJson(route, {
    items: statuses.map((status) => ({ status, count: status === "failed" ? 50 : 0 })),
    total: 50,
  }))
  await page.route("**/api/tasks?*", async (route) => {
    const items = Array.from({ length: 50 }, (_, index) => task(index, index === 0 ? {
      status: "failed",
      input_path: "/媒体/アニメ/最終章/非常に長い保存場所/劇場版-最終章-超長編成名-第一話.mkv",
      output_path: "/輸出/完成作品/劇場版-最終章-超長編成名-第一話.mp4",
      failure: { failure_stage: "processing", failure_code: "processing_failed", message: "處理節點回報長篇診斷：模型載入失敗，但完整訊息必須保持在表格邊界內。", retryable: true },
    } : {}))
    await fulfillJson(route, { items, total: items.length, limit: 50, offset: 0 })
  })
}

async function expectContained(page: Page, frame: Locator): Promise<void> {
  expect(await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth)).toBe(false)
  expect(await frame.evaluate((element) => element.scrollWidth > element.clientWidth)).toBe(true)
}

async function alignHeader(frame: Locator, header: Locator): Promise<void> {
  await header.evaluate((element) => {
    const scrollFrame = element.closest(".task-table-frame")
    if (!(scrollFrame instanceof HTMLElement)) throw new TypeError("task table frame is missing")
    const headerBounds = element.getBoundingClientRect()
    const frameBounds = scrollFrame.getBoundingClientRect()
    const target = scrollFrame.scrollLeft + headerBounds.left - frameBounds.left
    scrollFrame.scrollLeft = Math.max(0, Math.min(scrollFrame.scrollWidth - scrollFrame.clientWidth, target))
  })
  await expect.poll(() => frame.evaluate((element) => element.scrollLeft)).toBeGreaterThan(0)
}

async function capture(page: Page, name: string): Promise<void> {
  expect(await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth)).toBe(false)
  await page.screenshot({ path: `${evidenceDir}/${name}`, animations: "disabled", fullPage: false, scale: "css" })
}
