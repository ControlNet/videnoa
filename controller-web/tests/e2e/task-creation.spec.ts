import { expect, test } from "@playwright/test"

import { fulfillJson, installPagedApi, requestJournal, task, taskDetail } from "./tasks-fixtures"

test("creates and replays a task on plain HTTP without crypto.randomUUID", async ({ page, baseURL }) => {
  // Given: a real insecure browser origin serving the app with synthetic test-only API data.
  const origin = "http://controller-http.test"
  await page.route(`${origin}/**`, async (route) => {
    const url = new URL(route.request().url())
    const target = new URL(url.pathname + url.search, baseURL)
    target.hostname = "127.0.0.1"
    const response = await route.fetch({ url: target.href })
    await route.fulfill({ response })
  })
  await installPagedApi(page, requestJournal(), 1)
  const errors: string[] = []
  page.on("pageerror", (error) => errors.push(error.message))
  const keys: string[] = []
  const bodies: unknown[] = []
  const createdTask = task(0)
  await page.route(`**/api/tasks/${createdTask.id}?*`, async (route) => fulfillJson(route, taskDetail(createdTask)))
  await page.route("**/api/tasks", async (route) => {
    keys.push(route.request().headers()["idempotency-key"] ?? "")
    bodies.push(route.request().postDataJSON())
    if (keys.length === 1) await route.abort("connectionreset")
    else await fulfillJson(route, createdTask)
  })
  await page.goto(`${origin}/tasks`)
  expect(await page.evaluate(() => ({
    secureContext: window.isSecureContext,
    randomUUID: typeof crypto.randomUUID,
    getRandomValues: typeof crypto.getRandomValues,
  }))).toEqual({ secureContext: false, randomUUID: "undefined", getRandomValues: "function" })

  // When: task creation loses its response and the operator retries unchanged.
  await page.getByRole("button", { name: "Add Task" }).click()
  await page.getByRole("textbox", { name: "Input Path", exact: true }).fill(createdTask.input_path)
  await page.getByRole("textbox", { name: "Output Path", exact: true }).fill(createdTask.output_path)
  await page.getByLabel("Workflow").last().fill(createdTask.workflow)
  await page.getByRole("button", { name: "Create Task" }).click()
  await page.getByRole("button", { name: "Retry Same Task" }).click()

  // Then: the request reaches the API with a valid, stable UUID and no browser exception.
  await expect(page.getByRole("dialog")).toHaveCount(0)
  await expect(page.getByRole("region", { name: "Task Detail" })).toContainText(createdTask.input_path)
  expect(keys).toHaveLength(2)
  expect(keys[0]).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/)
  expect(keys[1]).toBe(keys[0])
  expect(bodies[1]).toEqual(bodies[0])
  expect(errors).toEqual([])
})

test("replays one manual creation intent after a dropped response", async ({ page }) => {
  // Given: the first create response drops after the server accepts the logical task.
  const journal = requestJournal()
  await installPagedApi(page, journal, 1)
  const keys: string[] = []
  const bodies: unknown[] = []
  let creates = 0
  const createdTask = task(0, {
    source: "manual",
    input_path: "/nas/input/Show/episode.01.mkv",
    output_path: "/nas/output/Show/episode.01.mp4",
    workflow: "anime-2x",
    priority: 17,
  })
  await page.route(`**/api/tasks/${createdTask.id}?*`, async (route) => fulfillJson(route, taskDetail(createdTask)))
  await page.route("**/api/tasks", async (route) => {
    creates += 1
    keys.push(route.request().headers()["idempotency-key"] ?? "")
    bodies.push(route.request().postDataJSON())
    if (creates === 1) {
      await route.abort("connectionreset")
      return
    }
    await fulfillJson(route, createdTask)
  })
  await page.goto("/tasks")

  // When: the operator submits exact paths, sees ambiguity, and retries unchanged.
  await page.getByRole("button", { name: "Add Task" }).click()
  expect(Number.parseFloat(await page.getByRole("button", { name: "Create Task" }).evaluate((element) => getComputedStyle(element).columnGap))).toBeGreaterThan(0)
  await page.getByRole("textbox", { name: "Input Path", exact: true }).fill("/nas/input/Show/episode.01.mkv")
  await page.getByRole("textbox", { name: "Output Path", exact: true }).fill("/nas/output/Show/episode.01.mp4")
  await page.getByLabel("Workflow").last().fill("anime-2x")
  await page.getByLabel("Priority").fill("17")
  await page.getByRole("button", { name: "Create Task" }).click()
  await expect(page.getByRole("alert")).toContainText("response")
  const replay = page.getByRole("button", { name: "Retry Same Task" })
  await replay.focus()
  await page.keyboard.press("Shift+Tab")
  await page.keyboard.press("Tab")
  await expect(replay).toBeFocused()
  await page.screenshot({
    path: "../.omo/evidence/videnoa-controller/task-19/playwright-report/screenshots/task-17/task-actions/manual-create-replay-keyboard-focus.png",
    animations: "disabled",
  })
  await replay.click()

  // Then: both requests carry one key and one exact explicit-manual body.
  await expect(page.getByRole("dialog")).toHaveCount(0)
  const detail = page.getByRole("region", { name: "Task Detail" })
  await expect(detail).toContainText("/nas/input/Show/episode.01.mkv")
  await expect(detail).toContainText("/nas/output/Show/episode.01.mp4")
  await expect(detail.locator("dt", { hasText: /^Input Path$/ }).locator("xpath=following-sibling::dd")).toHaveText("/nas/input/Show/episode.01.mkv")
  await expect(detail.locator("dt", { hasText: /^Output Path$/ }).locator("xpath=following-sibling::dd")).toHaveText("/nas/output/Show/episode.01.mp4")
  await expect(detail.locator("dt", { hasText: /^Workflow$/ }).locator("xpath=following-sibling::dd")).toHaveText("anime-2x")
  await expect(detail.locator("dt", { hasText: /^Priority$/ }).locator("xpath=following-sibling::dd")).toHaveText("17")
  await expect(detail.locator("dt", { hasText: /^Source$/ }).locator("xpath=following-sibling::dd")).toHaveText("manual")
  await detail.screenshot({
    path: "../.omo/evidence/videnoa-controller/task-19/playwright-report/screenshots/task-17/task-actions/manual-create-replay-success.png",
    animations: "disabled",
  })
  expect(keys).toHaveLength(2)
  expect(new Set(keys).size).toBe(1)
  expect(bodies[0]).toEqual(bodies[1])
  expect(bodies[1]).toEqual({
    input_path: "/nas/input/Show/episode.01.mkv",
    output_path: "/nas/output/Show/episode.01.mp4",
    workflow: "anime-2x",
    priority: 17,
    source: "manual",
    source_reference: null,
  })
})

test("uses a new key after an ambiguous form changes and recovers a key collision safely", async ({ page }) => {
  // Given: a dropped first response followed by a server-reported key/body collision.
  const journal = requestJournal()
  await installPagedApi(page, journal, 1)
  const keys: string[] = []
  let creates = 0
  await page.route("**/api/tasks", async (route) => {
    creates += 1
    keys.push(route.request().headers()["idempotency-key"] ?? "")
    if (creates === 1) {
      await route.abort("connectionreset")
      return
    }
    await fulfillJson(
      route,
      {
        error: {
          code: "conflict",
          message: "idempotency key was used with a different request body",
          retryable: false,
          field_errors: [],
        },
      },
      409,
    )
  })
  await page.goto("/tasks")
  await page.getByRole("button", { name: "Add Task" }).click()
  await page.getByRole("textbox", { name: "Input Path", exact: true }).fill("/nas/input/exact.mkv")
  await page.getByRole("textbox", { name: "Output Path", exact: true }).fill("/nas/output/first.mp4")
  await page.getByLabel("Workflow").last().fill("anime-2x")
  await page.getByRole("button", { name: "Create Task" }).click()
  await expect(page.getByRole("button", { name: "Retry Same Task" })).toBeVisible()

  // When: the output path changes and the changed intent receives a collision response.
  await page.getByRole("textbox", { name: "Output Path", exact: true }).fill("/nas/output/second.mp4")
  await page.getByRole("button", { name: "Create Task" }).click()

  // Then: the changed body never reuses the ambiguous key and the collision remains actionable.
  await expect(page.getByRole("alert")).toContainText("new intent")
  expect(keys).toHaveLength(2)
  expect(keys[1]).not.toBe(keys[0])
})

test("focuses structured workspace and output collision errors and restores the add trigger", async ({ page }) => {
  // Given: the server reports real structured intake failures under a generic top-level message.
  const journal = requestJournal()
  await installPagedApi(page, journal, 1)
  let creates = 0
  await page.route("**/api/tasks", async (route) => {
    creates += 1
    const fieldError = creates === 1
      ? { field: "input_path", code: "invalid_value", message: "path is outside the Controller workspace" }
      : { field: "output_path", code: "invalid_value", message: "output must not already exist" }
    await fulfillJson(route, { error: { code: "invalid_request", message: "request validation failed", retryable: false, field_errors: [fieldError] } }, 400)
  })
  await page.goto("/tasks")
  await page.getByRole("button", { name: "Add Task" }).click()
  await page.getByRole("textbox", { name: "Input Path", exact: true }).fill("/outside/input.mkv")
  await page.getByRole("textbox", { name: "Output Path", exact: true }).fill("/nas/output/existing.mp4")
  await page.getByLabel("Workflow").last().fill("anime-2x")

  // When: workspace validation fails, then corrected input reaches no-clobber validation.
  await page.getByRole("button", { name: "Create Task" }).click()
  await expect(page.getByRole("alert")).toContainText("outside the Controller workspace")
  await expect(page.getByRole("textbox", { name: "Input Path" })).toBeFocused()
  await page.screenshot({
    path: "../.omo/evidence/videnoa-controller/task-19/playwright-report/screenshots/task-17/task-actions/manual-intake-root-focus.png",
    animations: "disabled",
  })
  await page.getByRole("textbox", { name: "Input Path" }).fill("/nas/input/exact.mkv")
  await page.getByRole("button", { name: "Create Task" }).click()
  await expect(page.getByRole("alert")).toContainText("will not be overwritten")
  await expect(page.getByRole("textbox", { name: "Output Path" })).toBeFocused()
  await expect(page.getByRole("textbox", { name: "Output Path" })).toHaveAttribute("aria-describedby", "task-output-path-error")
  await page.keyboard.press("Escape")

  // Then: both failures were executed and the native modal restores its trigger.
  expect(creates).toBe(2)
  await expect(page.getByRole("dialog")).toHaveCount(0)
  await expect(page.getByRole("button", { name: "Add Task" })).toBeFocused()
})
