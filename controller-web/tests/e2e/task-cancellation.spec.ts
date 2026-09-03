import { expect, test } from "@playwright/test"

import { fulfillJson, installLiveApi, requestJournal, task } from "./tasks-fixtures"

test("keeps cancellation confirmation keyboard focus safe and contained", async ({ page }) => {
  // Given: an authoritative processing detail with cancellation available.
  const journal = requestJournal()
  const processing = task(4, { status: "processing", version: 3 })
  await installLiveApi(page, journal, processing)
  await page.route(`**/api/tasks/${processing.id}`, async (route) => fulfillJson(route, { task: processing, attempts: [] }))
  await page.goto("/tasks")
  const rowTrigger = page.getByRole("button", { name: /Open task/ })
  await rowTrigger.click()

  // When: the operator opens confirmation and navigates it entirely by keyboard.
  const cancelTask = page.getByRole("button", { name: "Cancel Task" })
  await cancelTask.click()
  const keepTask = page.getByRole("button", { name: "Keep Task" })
  const confirmCancellation = page.getByRole("button", { name: "Confirm Cancellation" })

  // Then: safe focus starts first and cycles only across confirmation actions.
  await expect(keepTask).toBeFocused()
  expect(await keepTask.evaluate((button) => getComputedStyle(button).outlineStyle)).toBe("solid")
  await page.screenshot({
    path: "../.omo/evidence/videnoa-controller/task-17/task-actions/cancel-confirmation-safe-focus.png",
    animations: "disabled",
  })
  await page.keyboard.press("Tab")
  await expect(confirmCancellation).toBeFocused()
  await page.keyboard.press("Tab")
  await expect(keepTask).toBeFocused()
  await page.keyboard.press("Shift+Tab")
  await expect(confirmCancellation).toBeFocused()

  // When: Escape dismisses the nested confirmation, then a second Escape closes detail.
  await page.keyboard.press("Escape")
  await expect(page.getByRole("alertdialog", { name: "Confirm Task Cancellation" })).toHaveCount(0)
  await expect(page.getByRole("region", { name: "Task Detail" })).toBeVisible()
  await expect(cancelTask).toBeFocused()
  await page.keyboard.press("Escape")

  // Then: detail closes only after confirmation is gone and focus returns to its row.
  await expect(page.getByRole("region", { name: "Task Detail" })).toHaveCount(0)
  await expect(rowTrigger).toBeFocused()
})

test("refetches one bounded page and counts with authoritative detail after a version conflict", async ({ page }) => {
  // Given: cancellation races with a newer task version carrying durable cancellation intent.
  const journal = requestJournal()
  const initial = task(5, { status: "processing", version: 7 })
  const current = task(5, { status: "processing", version: 8, cancel_requested_at: "2030-01-01T00:06:00Z" })
  await installLiveApi(page, journal, initial)
  let detail = initial
  let detailRequests = 0
  await page.route(`**/api/tasks/${initial.id}`, async (route) => {
    detailRequests += 1
    await fulfillJson(route, { task: detail, attempts: [] })
  })
  await page.route(`**/api/tasks/${initial.id}/cancel`, async (route) => {
    detail = current
    await fulfillJson(route, { error: { code: "conflict", message: "version conflict", retryable: true, field_errors: [] } }, 409)
  })
  await page.goto("/tasks")
  await page.getByRole("button", { name: /Open task/ }).click()
  const pageRequestsBeforeConflict = journal.tasks.length
  const countRequestsBeforeConflict = journal.counts.length

  // When: the current-version cancellation receives HTTP 409.
  await page.getByRole("button", { name: "Cancel Task" }).click()
  await page.getByRole("button", { name: "Confirm Cancellation" }).click()

  // Then: each authoritative surface refetches exactly once and repeated cancellation disappears.
  await expect(page.getByText(/review the new state before acting again/i)).toBeVisible()
  await expect(page.getByRole("region", { name: "Task Detail" }).getByText("8", { exact: true })).toBeVisible()
  await expect(page.getByRole("button", { name: "Cancel Task" })).toHaveCount(0)
  expect(detailRequests).toBe(2)
  expect(journal.tasks).toHaveLength(pageRequestsBeforeConflict + 1)
  expect(journal.counts).toHaveLength(countRequestsBeforeConflict + 1)
  expect(journal.tasks.at(-1)).toContain("limit=50")
  expect(journal.tasks.at(-1)).toContain("offset=0")
})

const blockedCancellationScenarios = [
  ["late", task(7, { status: "publishing" })],
  ["repeated", task(8, { status: "processing", cancel_requested_at: "2030-01-01T00:06:00Z" })],
] as const

for (const [caseName, authoritative] of blockedCancellationScenarios) {
  test(`blocks a ${caseName} cancellation from authoritative detail`, async ({ page }) => {
  // Given: the list row is active but authoritative detail forbids cancellation.
  const journal = requestJournal()
  const listed = task(7, { id: authoritative.id, status: "processing" })
  const cancellationRequests: string[] = []
  await installLiveApi(page, journal, listed)
  await page.route(`**/api/tasks/${listed.id}`, async (route) => fulfillJson(route, { task: authoritative, attempts: [] }))
  await page.route(`**/api/tasks/${listed.id}/cancel`, async (route) => {
    cancellationRequests.push(route.request().url())
    await fulfillJson(route, { error: { code: "conflict", message: "late cancellation", retryable: false, field_errors: [] } }, 409)
  })

  // When: the operator opens the selected task.
  await page.goto("/tasks")
  await page.getByRole("button", { name: /Open task/ }).click()

  // Then: no cancellation control or request is available.
  await expect(page.getByRole("button", { name: "Cancel Task" })).toHaveCount(0)
  expect(cancellationRequests).toEqual([])
  })
}
