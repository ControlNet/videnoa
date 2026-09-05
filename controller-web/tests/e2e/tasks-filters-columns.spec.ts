import { expect, test } from "@playwright/test"

import { capture, fulfillJson, installLiveApi, installPagedApi, requestJournal, task, taskDetail } from "./tasks-fixtures"

test("persists server-backed Source and Failure Stage filters with independent task columns", async ({ page }) => {
  // Given: bounded server data and every independent optional task column in the URL.
  const journal = requestJournal()
  await installPagedApi(page, journal, 280)
  await page.setViewportSize({ width: 1280, height: 900 })
  await page.emulateMedia({ colorScheme: "dark", reducedMotion: "reduce" })
  await page.goto(
    "/tasks?columns=input_path,output_path,attempts,duration,failure_stage,failure,error,remote_job_id",
  )
  await expect(page.getByRole("table").locator("tbody tr")).toHaveCount(50)
  const filters = page.getByRole("group", { name: "Task filters" })

  // When: the operator applies both server-backed filters.
  await filters.getByRole("combobox", { name: "Source" }).selectOption("manual")
  await filters.getByRole("combobox", { name: "Failure Stage" }).selectOption("processing")
  await expect.poll(() => journal.tasks.length).toBe(3)

  // Then: URL state and the actual bounded API request carry the same filters.
  await expect.poll(() => new URL(page.url()).searchParams.get("source")).toBe("manual")
  await expect.poll(() => new URL(page.url()).searchParams.get("failure_stage")).toBe("processing")
  const filteredRequest = new URLSearchParams(journal.tasks.at(-1))
  expect(filteredRequest.get("source")).toBe("manual")
  expect(filteredRequest.get("failure_stage")).toBe("processing")
  await expect(page.getByRole("table").locator("tbody tr")).toHaveCount(20)

  // Then: every requested column is independently named and the legacy Path label is absent.
  for (const label of [
    "Input Path",
    "Output Path",
    "Attempts",
    "Duration",
    "Failure Stage",
    "Failure",
    "Error",
    "Remote Job ID",
  ]) {
    await expect(page.getByRole("columnheader", { name: label, exact: true })).toBeAttached()
  }
  await expect(page.getByRole("columnheader", { name: "Path", exact: true })).toHaveCount(0)

  // When: only Output Path is disabled in the Columns control.
  const requestsBeforeColumnChange = journal.tasks.length
  await page.getByText("Columns", { exact: true }).click()
  const outputPathColumn = page.getByRole("checkbox", { name: "Show Output Path column" })
  await outputPathColumn.click()
  await expect(outputPathColumn).not.toBeChecked()

  // Then: Input Path remains, Output Path disappears, and client-only column state does not refetch.
  await expect(page.getByRole("columnheader", { name: "Input Path", exact: true })).toBeAttached()
  await expect(page.getByRole("columnheader", { name: "Output Path", exact: true })).toHaveCount(0)
  expect(new URL(page.url()).searchParams.get("columns")?.split(",")).not.toContain("output_path")
  expect(journal.tasks).toHaveLength(requestsBeforeColumnChange)

  // When: the shareable route is reloaded.
  await page.reload()

  // Then: filters and the independent column selection are restored from the URL.
  await expect(filters.getByRole("combobox", { name: "Source" })).toHaveValue("manual")
  await expect(filters.getByRole("combobox", { name: "Failure Stage" })).toHaveValue("processing")
  await expect(page.getByRole("columnheader", { name: "Input Path", exact: true })).toBeAttached()
  await expect(page.getByRole("columnheader", { name: "Output Path", exact: true })).toHaveCount(0)
})

test("keeps Error and all eight column toggles clickable above the task detail header with one Chinese task", async ({ page }) => {
  // Given: one Chinese task keeps the detail header directly beneath the open column picker.
  const journal = requestJournal()
  const selectedTask = task(0, {
    input_path: "/媒体/動畫/最終章/單列控制器任務.mkv",
    output_path: "/輸出/動畫/最終章/單列控制器任務.mp4",
  })
  await installLiveApi(page, journal, selectedTask)
  await page.route(`**/api/tasks/${selectedTask.id}?*`, async (route) => fulfillJson(route, taskDetail(selectedTask)))
  await page.setViewportSize({ width: 1440, height: 900 })
  await page.goto("/tasks?source=manual")
  await expect(page.getByRole("table").locator("tbody tr")).toHaveCount(1)
  await page.getByRole("button", { name: `Open task ${selectedTask.id}` }).click()
  await expect(page.getByRole("region", { name: "Task Detail" })).toBeVisible()

  // When: the operator enables every independent option through the overlapping picker.
  await page.getByText("Columns", { exact: true }).click()
  const optionalColumns = [
    "Input Path",
    "Output Path",
    "Attempts",
    "Duration",
    "Failure Stage",
    "Failure",
    "Error",
    "Remote Job ID",
  ] as const
  for (const label of optionalColumns) {
    const checkbox = page.getByRole("checkbox", { name: `Show ${label} column` })
    await checkbox.click()
    await expect(checkbox).toBeChecked()
    await expect(page.getByRole("columnheader", { name: label, exact: true })).toBeAttached()
  }

  // Then: all eight clicks are accepted, persisted, and visible with detail still open.
  await expect.poll(() => new URL(page.url()).searchParams.get("columns")).toBe(
    "input_path,output_path,attempts,duration,failure_stage,failure,error,remote_job_id",
  )
  await expect(page.getByRole("region", { name: "Task Detail" })).toBeVisible()
  await capture(page, "columns-one-row-chinese-detail.png")
})
