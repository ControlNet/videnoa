import { expect, test } from "@playwright/test"

import { dispatchTaskUpdate, fulfillJson, installLiveApi, requestJournal, task, taskDetail } from "./tasks-fixtures"

test("opens authoritative detail and sends current versions for cancel and retry", async ({ page }) => {
  // Given: a processing row with persisted attempt detail and action routes.
  await page.setViewportSize({ width: 1280, height: 900 })
  const journal = requestJournal()
  const processing = task(0, { status: "processing", version: 7 })
  await installLiveApi(page, journal, processing)
  const versions: number[] = []
  await page.route(`**/api/tasks/${processing.id}?*`, async (route) =>
    fulfillJson(route, taskDetail(processing, [
        {
          id: "00000000-0000-4000-8000-000000000002",
          task_id: processing.id,
          attempt_number: 1,
          worker_id: processing.worker_id,
          status: "processing",
          submission_key: "00000000-0000-4000-8000-000000000004",
          remote_job_id: processing.remote_job_id,
          remote_input_path: "task/input/opaque.mkv",
          remote_output_path: "task/output/opaque.mp4",
          progress: processing.progress,
          retry: { retry_count: 0, next_retry_at: null },
          failure: null,
          created_at: processing.created_at,
          started_at: processing.updated_at,
          completed_at: null,
        },
      ])),
  )
  await page.route(`**/api/tasks/${processing.id}/cancel`, async (route) => {
    versions.push(route.request().postDataJSON().version)
    await fulfillJson(route, {
      task_id: processing.id,
      status: "processing",
      cancel_requested_at: processing.updated_at,
    })
  })
  await page.goto("/tasks")

  // When: the row is selected and the current task is cancelled.
  await page
    .getByRole("button", { name: /Open task/ })
    .first()
    .click()
  await expect(page.getByRole("region", { name: "Task Detail" })).toContainText(processing.input_path)
  await expect(page.getByRole("region", { name: "Task Detail" })).toContainText("Attempt 1")
  const pane = page.locator(".task-detail-pane")
  await pane.scrollIntoViewIfNeeded()
  await pane.evaluate((element) => element.scrollTo({ top: 0 }))
  await page.screenshot({
    path: "../.omo/evidence/videnoa-controller/task-19/playwright-report/screenshots/task-17/task-actions/detail-general-progress.png",
    animations: "disabled",
  })
  await pane.evaluate((element) => {
    const attempt = element.querySelector(".attempt-section")
    if (attempt instanceof HTMLElement) element.scrollTo({ top: attempt.offsetTop })
  })
  await page.screenshot({
    path: "../.omo/evidence/videnoa-controller/task-19/playwright-report/screenshots/task-17/task-actions/detail-attempts.png",
    animations: "disabled",
  })
  await page.getByRole("button", { name: "Cancel Task" }).click()
  const cancellation = page.waitForResponse(`**/api/tasks/${processing.id}/cancel`)
  await page.getByRole("button", { name: "Confirm Cancellation" }).click()
  await cancellation

  // Then: the detail route, persisted attempt, and optimistic version are authoritative.
  expect(versions).toEqual([7])
})

test("shows retry only for explicit safe failure evidence", async ({ page }) => {
  // Given: a retryable processing failure with authoritative attempt history.
  const journal = requestJournal()
  const failed = task(12, {
    status: "failed",
    version: 9,
    failure: {
      failure_stage: "processing",
      failure_code: "processing_failed",
      message: "remote processing failed",
      retryable: true,
    },
  })
  await installLiveApi(page, journal, failed)
  let retriedVersion: number | null = null
  await page.route(`**/api/tasks/${failed.id}?*`, async (route) => fulfillJson(route, taskDetail(failed)))
  await page.route(`**/api/tasks/${failed.id}/retry`, async (route) => {
    retriedVersion = route.request().postDataJSON().version
    await fulfillJson(route, {
      task_id: failed.id,
      attempt_id: "00000000-0000-4000-8000-000000000099",
      status: "queued",
    })
  })
  await page.goto("/tasks")

  // When: the failed task is opened and retry is accepted.
  await page.getByRole("button", { name: /Open task/ }).click()
  const guidance = page.getByText(/remote job is terminal/)
  await expect(guidance).toBeVisible()
  await guidance.scrollIntoViewIfNeeded()
  const retry = page.getByRole("button", { name: "Retry Failed Stage" })
  await retry.focus()
  await page.screenshot({
    path: "../.omo/evidence/videnoa-controller/task-19/playwright-report/screenshots/task-17/task-actions/safe-processing-retry.png",
    animations: "disabled",
  })
  await retry.click()

  // Then: the current optimistic version is sent.
  expect(retriedVersion).toBe(9)
})

test("blocks retry for ambiguous failure evidence", async ({ page }) => {
  // Given: a failed task whose API evidence is explicitly ambiguous.
  const journal = requestJournal()
  const ambiguous = task(12, {
    status: "failed",
    failure: {
      failure_stage: "processing",
      failure_code: "remote_state_ambiguous",
      message: "remote response was incomplete",
      retryable: false,
    },
  })
  await installLiveApi(page, journal, ambiguous)
  await page.route(`**/api/tasks/${ambiguous.id}?*`, async (route) => fulfillJson(route, taskDetail(ambiguous)))
  await page.goto("/tasks")

  // When: the authoritative failure detail is opened.
  await page.getByRole("button", { name: /Open task/ }).click()

  // Then: no retry action is exposed and manual remote verification guidance is visible.
  await expect(page.getByRole("button", { name: "Retry Failed Stage" })).toHaveCount(0)
  await expect(page.getByText(/Verify the remote job and workspace manually/)).toBeVisible()
})

test("refetches authoritative detail after a selected task event", async ({ page }) => {
  // Given: the selected task detail starts at version 4.
  const journal = requestJournal()
  const initial = task(5, { status: "processing", version: 4 })
  const updated = task(5, {
    status: "processing",
    version: 5,
    progress: { ...initial.progress, percent: 61 },
  })
  await installLiveApi(page, journal, initial)
  let detail = initial
  let detailRequests = 0
  await page.route(`**/api/tasks/${initial.id}?*`, async (route) => {
    detailRequests += 1
    await fulfillJson(route, taskDetail(detail))
  })
  await page.goto("/tasks")
  await page.getByRole("button", { name: /Open task/ }).click()
  await expect(page.getByRole("region", { name: "Task Detail" }).getByText("4", { exact: true })).toBeVisible()

  // When: SSE reports a newer representation for the selected task.
  detail = updated
  await dispatchTaskUpdate(page, updated)

  // Then: detail is fetched again instead of trusting the list event as authoritative.
  await expect(page.getByRole("region", { name: "Task Detail" })).toContainText("61%")
  expect(detailRequests).toBeGreaterThanOrEqual(2)
})

test("keeps task actions contained at narrow viewport width", async ({ page }) => {
  // Given: a narrow operator viewport with ambiguous failure detail.
  await page.setViewportSize({ width: 420, height: 780 })
  const journal = requestJournal()
  const ambiguous = task(12, {
    status: "failed",
    failure: {
      failure_stage: "processing",
      failure_code: "publication_ambiguous",
      message: "destination state is uncertain",
      retryable: false,
    },
  })
  await installLiveApi(page, journal, ambiguous)
  await page.route(`**/api/tasks/${ambiguous.id}?*`, async (route) => fulfillJson(route, taskDetail(ambiguous)))

  // When: the table and bottom detail pane are rendered.
  await page.goto("/tasks")
  await page.getByRole("button", { name: /Open task/ }).click()

  // Then: the document does not overflow horizontally and blocked guidance remains visible.
  await expect(page.getByText(/Inspect the destination and staging artifact/)).toBeVisible()
  await page.getByText(/Inspect the destination and staging artifact/).scrollIntoViewIfNeeded()
  await page.screenshot({
    path: "../.omo/evidence/videnoa-controller/task-19/playwright-report/screenshots/task-17/task-actions/narrow-ambiguous-retry-blocked.png",
    animations: "disabled",
  })
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(420)
})
