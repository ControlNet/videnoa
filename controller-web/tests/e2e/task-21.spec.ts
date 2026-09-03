import AxeBuilder from "@axe-core/playwright"
import { expect, test } from "@playwright/test"

import type { Task } from "../../src/api/taskSchemas"
import {
  alignDetailLabelBelowHeader,
  alignDetailPaneInViewport,
  alignDetailPaneToEnd,
  attempt,
  captureTask21,
  createGate,
  detailPage,
  expectDetailEndReachable,
  installAbortResistantHistoryFetch,
  resetTask21Screenshots,
} from "./task-21.fixtures"
import {
  dispatchTaskUpdate,
  fulfillJson,
  installLiveApi,
  installPagedApi,
  requestJournal,
  task,
} from "./tasks-fixtures"

test.beforeEach(async ({ page }) => {
  await page.emulateMedia({ colorScheme: "dark", reducedMotion: "reduce" })
})

test("keeps delayed task A history out of newly selected task B", async ({ page }) => {
  // Given: task A history is held after the operator requests its next page.
  const journal = requestJournal()
  const staleGate = createGate()
  const taskA = task(0, { attempt_count: 2 })
  const taskB = task(1, { attempt_count: 1 })
  const taskANewest = attempt(taskA, 2)
  const taskAOlder = attempt(taskA, 1)
  const taskBOnly = attempt(taskB, 1)
  let heldHistoryRequests = 0
  await installAbortResistantHistoryFetch(page)
  await installPagedApi(page, journal, 2)
  await page.route(`**/api/tasks/${taskA.id}?*`, async (route) => {
    const offset = Number(new URL(route.request().url()).searchParams.get("offset"))
    if (offset === 1) {
      heldHistoryRequests += 1
      await staleGate.wait
    }
    await fulfillJson(route, offset === 0 ? detailPage(taskA, [taskANewest], 2) : detailPage(taskA, [taskAOlder], 2, 1))
  })
  await page.route(`**/api/tasks/${taskB.id}?*`, async (route) => fulfillJson(route, detailPage(taskB, [taskBOnly], 1)))
  await page.goto("/tasks")
  await page.getByRole("button", { name: `Open task ${taskA.id}` }).click()
  await page.getByRole("button", { name: "Load more attempts" }).click()
  await expect.poll(() => heldHistoryRequests).toBe(1)

  // When: task B loads before task A's abort-resistant history response resolves.
  await page.getByRole("button", { name: `Open task ${taskB.id}` }).click()
  await expect(page.getByRole("region", { name: "Task Detail" })).toContainText(taskB.id)
  staleGate.release()

  // Then: only task B remains authoritative in the detail inspector.
  const detailPane = page.getByRole("region", { name: "Task Detail" })
  await expect(detailPane).toContainText(taskB.id)
  await expect(detailPane).not.toContainText(taskAOlder.id)
  await expect(detailPane.getByRole("status")).toHaveText("Showing 1 of 1 persisted attempts.")
})

test("rejects reordered history after an SSE detail generation reload", async ({ page }) => {
  // Given: stale history and the replacement detail request can be released independently.
  const journal = requestJournal()
  const staleGate = createGate()
  const replacementGate = createGate()
  const originalTask = task(0, { attempt_count: 2, version: 1 })
  const updatedTask = { ...originalTask, version: 2 } satisfies Task
  const newest = attempt(updatedTask, 2)
  const older = attempt(updatedTask, 1)
  let initialRequests = 0
  let historyRequests = 0
  await installAbortResistantHistoryFetch(page)
  const liveApi = await installLiveApi(page, journal, originalTask)
  await page.route(`**/api/tasks/${originalTask.id}?*`, async (route) => {
    const offset = Number(new URL(route.request().url()).searchParams.get("offset"))
    if (offset === 1) {
      historyRequests += 1
      await staleGate.wait
      await fulfillJson(route, detailPage(originalTask, [newest, older], 2, 1))
      return
    }
    initialRequests += 1
    if (initialRequests === 2) await replacementGate.wait
    await fulfillJson(route, detailPage(initialRequests === 1 ? originalTask : updatedTask, initialRequests === 1 ? [newest] : [newest, older], 2))
  })
  await page.goto("/tasks")
  await page.getByRole("button", { name: `Open task ${originalTask.id}` }).click()
  await page.getByRole("button", { name: "Load more attempts" }).click()
  await expect.poll(() => historyRequests).toBe(1)

  // When: SSE invalidates detail and its replacement resolves before stale history.
  liveApi.setTask(updatedTask)
  await dispatchTaskUpdate(page, updatedTask)
  await expect.poll(() => initialRequests).toBe(2)
  replacementGate.release()
  await expect(page.getByText("Version").locator("..")).toContainText("2")
  staleGate.release()

  // Then: the newer detail stays intact with unique, coherent attempts.
  await expect(page.getByRole("region", { name: "Task Detail" }).getByRole("status")).toHaveText("Showing 2 of 2 persisted attempts.")
  await expect(page.getByText("Attempt 2", { exact: true })).toHaveCount(1)
  await expect(page.getByText("Attempt 1", { exact: true })).toHaveCount(1)
  await expect(page.getByText("Version").locator("..")).toContainText("2")
})

test("keeps history failure prominent and supports keyboard retry", async ({ page }) => {
  // Given: the first history page fails and the next attempt succeeds.
  const journal = requestJournal()
  const selectedTask = task(0, { attempt_count: 2 })
  const newest = attempt(selectedTask, 2)
  const older = attempt(selectedTask, 1)
  let initialRequests = 0
  let historyRequests = 0
  await installPagedApi(page, journal, 1)
  await page.route(`**/api/tasks/${selectedTask.id}?*`, async (route) => {
    const offset = Number(new URL(route.request().url()).searchParams.get("offset"))
    if (offset === 0) {
      initialRequests += 1
      await fulfillJson(route, detailPage(selectedTask, [newest], 2))
      return
    }
    historyRequests += 1
    if (historyRequests === 1) {
      await fulfillJson(route, { error: { code: "unavailable", message: "Attempt history is temporarily unavailable.", retryable: true, field_errors: [] } }, 503)
      return
    }
    await fulfillJson(route, detailPage(selectedTask, [older], 2, 1))
  })
  await page.goto("/tasks")
  await page.getByRole("button", { name: `Open task ${selectedTask.id}` }).click()

  // When: loading fails, the operator retries detail with the keyboard and requests history again.
  await page.getByRole("button", { name: "Load more attempts" }).click()
  const alert = page.getByRole("alert")
  await expect(alert).toContainText("Attempt history is temporarily unavailable.")
  const retry = page.getByRole("button", { name: "Retry Detail" })
  await retry.focus()
  await expect(retry).toBeFocused()
  await retry.press("Enter")
  await expect.poll(() => initialRequests).toBe(2)
  await page.getByRole("button", { name: "Load more attempts" }).click()

  // Then: the recovered page is announced and the assertive error clears.
  await expect(page.getByRole("region", { name: "Task Detail" }).getByRole("status")).toHaveText("Showing 2 of 2 persisted attempts.")
  await expect(alert).toHaveCount(0)
})

test("coalesces repeated activation and deduplicates overlapping history", async ({ page }) => {
  // Given: the next page contains an attempt already present in the inspector.
  const journal = requestJournal()
  const selectedTask = task(0, { attempt_count: 2 })
  const newest = attempt(selectedTask, 2)
  const older = attempt(selectedTask, 1)
  let historyRequests = 0
  await installPagedApi(page, journal, 1)
  await page.route(`**/api/tasks/${selectedTask.id}?*`, async (route) => {
    const offset = Number(new URL(route.request().url()).searchParams.get("offset"))
    if (offset === 0) {
      await fulfillJson(route, detailPage(selectedTask, [newest], 2))
      return
    }
    historyRequests += 1
    await fulfillJson(route, detailPage(selectedTask, [newest, older], 2, 1))
  })
  await page.goto("/tasks")
  await page.getByRole("button", { name: `Open task ${selectedTask.id}` }).click()
  const loadMore = page.getByRole("button", { name: "Load more attempts" })

  // When: pointer activation repeats synchronously before the first request can settle.
  await loadMore.evaluate((element) => {
    if (!(element instanceof HTMLButtonElement)) throw new TypeError("load-more control is not a button")
    element.click()
    element.click()
  })

  // Then: one request produces one copy of each attempt and an accessible settled count.
  await expect(page.getByRole("region", { name: "Task Detail" }).getByRole("status")).toHaveText("Showing 2 of 2 persisted attempts.")
  await expect(page.getByText("Attempt 2", { exact: true })).toHaveCount(1)
  await expect(page.getByText("Attempt 1", { exact: true })).toHaveCount(1)
  expect(historyRequests).toBe(1)
})

test("preserves accessible desktop and narrow detail layouts without browser secrets", async ({ page, context }) => {
  // Given: long CJK and technical history is rendered through the production preview.
  const journal = requestJournal()
  const historyGate = createGate()
  const selectedTask = task(0, {
    attempt_count: 2,
    input_path: "/nas/入力/超高解像度-長編アニメーション-最終検査版-episode-00001.mkv",
    output_path: "/nas/出力/超高解像度-長編アニメーション-最終検査版-episode-00001.mp4",
    source_reference: "外部-連携-参照-識別子-00000000000000000021",
  })
  const newest = attempt(selectedTask, 2, {
    remote_input_path: "/remote/入力/作業領域/非常に-長い-原本-経路/episode-00001.mkv",
    remote_output_path: "/remote/出力/作業領域/非常に-長い-結果-経路/episode-00001.mp4",
  })
  const older = attempt(selectedTask, 1)
  let historyRequests = 0
  await context.addCookies([{ name: "videnoa_session", value: crypto.randomUUID(), url: "http://127.0.0.1:4181", httpOnly: true, sameSite: "Strict" }])
  await installPagedApi(page, journal, 1)
  await page.route(`**/api/tasks/${selectedTask.id}?*`, async (route) => {
    const offset = Number(new URL(route.request().url()).searchParams.get("offset"))
    if (offset === 1) {
      historyRequests += 1
      await historyGate.wait
    }
    await fulfillJson(route, offset === 0 ? detailPage(selectedTask, [newest], 2) : detailPage(selectedTask, [older], 2, 1))
  })
  await page.goto("/tasks")
  await resetTask21Screenshots()
  const tableFrame = page.getByRole("region", { name: "Scrollable task results" })
  await tableFrame.scrollIntoViewIfNeeded()
  expect(await tableFrame.evaluate((element) => {
    const bounds = element.getBoundingClientRect()
    return {
      contained: bounds.left >= 0 && bounds.right <= window.innerWidth,
      documentOverflow: document.documentElement.scrollWidth > document.documentElement.clientWidth,
      ownsHorizontalOverflow: element.scrollWidth > element.clientWidth && getComputedStyle(element).overflowX === "auto",
    }
  })).toEqual({ contained: true, documentOverflow: false, ownsHorizontalOverflow: true })
  await captureTask21(page, "task-table-desktop-left.png")
  await tableFrame.evaluate((element) => { element.scrollLeft = element.scrollWidth - element.clientWidth })
  expect(await tableFrame.evaluate((element) => element.scrollLeft)).toBeGreaterThan(0)
  await captureTask21(page, "task-table-desktop-right.png")
  await page.getByRole("button", { name: `Open task ${selectedTask.id}` }).click()
  const loadMore = page.getByRole("button", { name: "Load more attempts" })
  const detailPane = page.getByRole("region", { name: "Task Detail" })
  const errorHeading = detailPane.getByText("Error / Logs", { exact: true })
  const finalLogLine = detailPane.getByText(
    "No persisted failure. The Controller API does not expose server logs for this task.",
    { exact: true },
  )

  // When: loading, settled history, keyboard focus, scroll, and 375px reflow are exercised.
  await alignDetailPaneInViewport(page)
  await loadMore.focus()
  await page.keyboard.press("Shift+Tab")
  await page.keyboard.press("Tab")
  await expect(loadMore).toBeFocused()
  expect(await loadMore.evaluate((element) => getComputedStyle(element).outlineStyle)).toBe("solid")
  await alignDetailLabelBelowHeader(page, "Remote Input", false)
  await alignDetailPaneToEnd(page)
  await expect(page.getByText("Remote Input", { exact: true }).first()).toBeInViewport()
  await expect(loadMore).toBeInViewport()
  await expect(errorHeading).toBeInViewport()
  await expect(finalLogLine).toBeInViewport()
  await expectDetailEndReachable(page)
  await captureTask21(page, "task-detail-desktop-focus.png", detailPane)
  await loadMore.press("Enter")
  await expect.poll(() => historyRequests).toBe(1)
  await expect(detailPane.getByRole("status")).toHaveText("Loading more persisted attempts…")
  await expect(page.getByRole("button", { name: "Loading attempts…" })).toBeDisabled()
  await alignDetailPaneToEnd(page)
  await expect(finalLogLine).toBeInViewport()
  await expectDetailEndReachable(page)
  await captureTask21(page, "task-detail-desktop-loading.png", detailPane)
  historyGate.release()
  await expect(detailPane.getByRole("status")).toHaveText("Showing 2 of 2 persisted attempts.")
  await page.setViewportSize({ width: 375, height: 812 })
  await detailPane.evaluate((element) => { element.scrollTop = 0 })
  await detailPane.scrollIntoViewIfNeeded()
  await expect(detailPane.getByText("General", { exact: true })).toBeInViewport()
  await captureTask21(page, "task-detail-375-top.png")
  await alignDetailPaneToEnd(page)
  await expect(detailPane.getByRole("status")).toBeInViewport()
  await expect(errorHeading).toBeInViewport()
  await expect(finalLogLine).toBeInViewport()
  await expectDetailEndReachable(page)
  expect(await detailPane.evaluate((element) => {
    const header = element.querySelector(".task-detail-header")
    if (!(header instanceof HTMLElement)) throw new TypeError("task detail header is missing")
    const paneRect = element.getBoundingClientRect()
    const headerRect = header.getBoundingClientRect()
    return headerRect.top >= paneRect.top && headerRect.bottom <= paneRect.bottom
  })).toBe(true)
  await captureTask21(page, "task-detail-375-history.png")

  // Then: the live region, local scroll owner, accessibility scan, and browser stores stay clean.
  await expect(page.getByRole("region", { name: "Task Detail" })).toContainText("超高解像度")
  expect((await new AxeBuilder({ page }).include(".task-detail-pane").analyze()).violations).toEqual([])
  expect(await page.evaluate(async () => ({
    caches: await caches.keys(),
    indexedDatabases: await indexedDB.databases(),
    localStorage: Object.keys(localStorage),
    sessionStorage: Object.keys(sessionStorage),
  }))).toEqual({ caches: [], indexedDatabases: [], localStorage: [], sessionStorage: [] })
  expect((await context.cookies()).map(({ httpOnly, name, sameSite }) => ({ httpOnly, name, sameSite }))).toEqual([
    { httpOnly: true, name: "videnoa_session", sameSite: "Strict" },
  ])
})
