import { expect, test } from "@playwright/test"
import AxeBuilder from "@axe-core/playwright"

import { fulfillJson, installPagedApi, requestJournal, task } from "./tasks-fixtures"

// Synthetic API responses exercise UI behavior without submitting real media jobs.
const requests = [1, 2].map((index) => ({
  input_path: `/media/Series/E0${index}.mkv`, output_path: `/media/Series/E0${index}.AI.mkv`,
  workflow: "anime", priority: 0, source: "manual", source_reference: null,
}))

test("previews a batch and retries only remaining tasks using stable keys on HTTP", async ({ page }) => {
  await installPagedApi(page, requestJournal(), 1)
  await page.addInitScript(() => { Object.defineProperty(crypto, "randomUUID", { value: undefined }) })
  const previews: unknown[] = []
  await page.route("**/api/tasks/batch-preview", async (route) => {
    previews.push(route.request().postDataJSON())
    await fulfillJson(route, { items: requests.map((request) => ({ request, error: null })) })
  })
  const submissions: { key: string; body: unknown }[] = []
  await page.route("**/api/tasks", async (route) => {
    submissions.push({ key: route.request().headers()["idempotency-key"] ?? "", body: route.request().postDataJSON() })
    if (submissions.length === 2) await route.abort("connectionreset")
    else await fulfillJson(route, task(submissions.length))
  })
  await page.goto("/tasks")
  await page.getByRole("button", { name: "Add Batch", exact: true }).click()
  const dialog = page.getByRole("dialog")
  await expect(dialog).toHaveAccessibleName("Add Batch")
  await expect(dialog.getByRole("button", { name: "Create Tasks" })).toHaveCount(0)
  await dialog.getByRole("combobox", { name: "Input Pattern", exact: true }).fill("/media/**/*.mkv")
  await dialog.getByRole("combobox", { name: "Workflow", exact: true }).fill("anime")
  await dialog.getByRole("button", { name: "Preview Tasks", exact: true }).click()
  await expect(dialog).toHaveAccessibleName("Preview Tasks")
  await expect(dialog.getByRole("heading", { name: "Preview Tasks" })).toBeFocused()
  await expect(dialog.getByRole("combobox")).toHaveCount(0)
  await expect(dialog.getByRole("status")).toContainText("2 matching files")
  await expect(dialog.getByRole("row")).toHaveCount(3)
  expect(submissions).toHaveLength(0)
  expect(previews[0]).toMatchObject({ input_pattern: "/media/**/*.mkv", output_mode: "beside_input", middle_extension: "AI" })
  expect((await new AxeBuilder({ page }).include('dialog[open]').analyze()).violations).toEqual([])
  await page.screenshot({ path: "../.omo/evidence/controller-batch-tasks/preview-desktop.png" })
  await dialog.getByRole("button", { name: "Create Tasks" }).click()
  await expect(dialog.getByRole("status")).toContainText("1 of 2 tasks created")
  await expect(dialog.getByRole("button", { name: "Back", exact: true })).toBeDisabled()
  await dialog.getByRole("button", { name: "Retry Remaining" }).click()
  await expect(dialog.getByRole("status")).toContainText("2 of 2 tasks created")
  expect(submissions).toHaveLength(3)
  expect(submissions[1]).toEqual(submissions[2])
  expect(submissions[0]?.key).not.toBe(submissions[1]?.key)
  await dialog.getByRole("button", { name: "Done" }).click()
  await expect(dialog).toHaveCount(0)
  await expect(page.getByRole("button", { name: "Add Batch", exact: true })).toBeFocused()
})

test("invalidates stale previews, restricts original names, and blocks collisions on mobile", async ({ page }) => {
  await page.setViewportSize({ width: 375, height: 812 })
  await installPagedApi(page, requestJournal(), 1)
  let previews = 0
  await page.route("**/api/tasks/batch-preview", async (route) => {
    previews += 1
    await fulfillJson(route, { items: previews === 3 ? [] : requests.map((request) => ({ request, error: previews === 2 ? "Multiple inputs map to this output path." : null })) })
  })
  await page.goto("/tasks")
  await page.getByRole("button", { name: "Add Batch", exact: true }).click()
  const dialog = page.getByRole("dialog")
  await expect(dialog.getByRole("option", { name: "Original filename" })).toHaveJSProperty("disabled", true)
  await dialog.getByRole("combobox", { name: "Output Mode" }).selectOption("directory")
  await dialog.getByRole("combobox", { name: "Output Directory", exact: true }).fill("/output/")
  await dialog.getByRole("combobox", { name: "Filename Format" }).selectOption("original")
  await expect(dialog.getByLabel("Middle Extension")).toHaveCount(0)
  await dialog.getByRole("combobox", { name: "Input Pattern", exact: true }).fill("/media/*.mkv")
  await dialog.getByRole("combobox", { name: "Workflow", exact: true }).fill("anime")
  await dialog.getByRole("button", { name: "Preview Tasks", exact: true }).click()
  await expect(dialog.getByRole("button", { name: "Create Tasks" })).toBeEnabled()
  await dialog.getByRole("button", { name: "Back", exact: true }).click()
  await expect(dialog).toHaveAccessibleName("Add Batch")
  await expect(dialog.getByRole("combobox", { name: "Input Pattern", exact: true })).toBeFocused()
  await expect(dialog.getByRole("combobox", { name: "Output Directory", exact: true })).toHaveValue("/output/")
  await expect(dialog.getByRole("combobox", { name: "Filename Format" })).toHaveValue("original")
  await dialog.getByLabel("Priority").fill("7")
  await expect(dialog.getByRole("table")).toHaveCount(0)
  await expect(dialog.getByRole("button", { name: "Create Tasks" })).toHaveCount(0)
  await dialog.getByRole("button", { name: "Preview Tasks", exact: true }).click()
  await expect(dialog.getByRole("status")).toContainText("2 conflicts")
  await expect(dialog.getByRole("button", { name: "Create Tasks" })).toBeDisabled()
  expect(await dialog.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(true)
  await page.screenshot({ path: "../.omo/evidence/controller-batch-tasks/preview-mobile.png" })
  await dialog.getByRole("button", { name: "Back", exact: true }).click()
  await dialog.getByRole("button", { name: "Preview Tasks", exact: true }).click()
  await expect(dialog).toContainText("No files matched")
  await expect(dialog.getByRole("button", { name: "Create Tasks" })).toBeDisabled()
})


test("keeps preview errors in the input step and previews through keyboard submission", async ({ page }) => {
  await installPagedApi(page, requestJournal(), 1)
  let previews = 0
  await page.route("**/api/tasks/batch-preview", async (route) => {
    previews += 1
    if (previews === 1) await route.abort("connectionreset")
    else await fulfillJson(route, { items: requests.map((request) => ({ request, error: null })) })
  })
  let submissions = 0
  await page.route("**/api/tasks", async (route) => { submissions += 1; await fulfillJson(route, task(1)) })
  await page.goto("/tasks")
  await page.getByRole("button", { name: "Add Batch", exact: true }).click()
  const dialog = page.getByRole("dialog")
  await dialog.getByRole("combobox", { name: "Input Pattern", exact: true }).fill("/media/*.mkv")
  await dialog.getByRole("combobox", { name: "Workflow", exact: true }).fill("anime")
  await dialog.getByLabel("Priority").press("Enter")
  await expect(dialog.getByRole("alert")).toBeVisible()
  await expect(dialog).toHaveAccessibleName("Add Batch")
  await expect(dialog.getByRole("button", { name: "Create Tasks" })).toHaveCount(0)
  await expect(dialog.getByRole("button", { name: "Preview Tasks", exact: true })).toBeEnabled()
  await dialog.getByLabel("Priority").press("Enter")
  await expect(dialog).toHaveAccessibleName("Preview Tasks")
  expect(submissions).toBe(0)
  await expect(dialog.getByRole("button", { name: "Create Tasks" })).toBeEnabled()
})
