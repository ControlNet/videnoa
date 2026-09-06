import AxeBuilder from "@axe-core/playwright"
import { expect, test } from "@playwright/test"

import { fulfillJson, installPagedApi, requestJournal, task, taskDetail } from "./tasks-fixtures"
import { installOperationalReadRoutes } from "./operations-fixtures"

for (const width of [1280, 390]) {
  test(`completes paths and workflows over plain HTTP at ${width}px`, async ({ page, baseURL }) => {
    // Real insecure browser origin, synthetic directory and Worker API responses.
    await page.setViewportSize({ width, height: 900 })
    const origin = "http://controller-http.test"
    await page.route(`${origin}/**`, async (route) => {
      const url = new URL(route.request().url())
      const target = new URL(url.pathname + url.search, baseURL)
      target.hostname = "127.0.0.1"
      await route.fulfill({ response: await route.fetch({ url: target.href }) })
    })
    await installPagedApi(page, requestJournal(), 1)
    await installOperationalReadRoutes(page)
    const errors: string[] = []
    page.on("pageerror", (error) => errors.push(error.message))
    await page.route("**/api/task-path-suggestions?*", async (route) => {
      const query = new URL(route.request().url()).searchParams
      const prefix = query.get("prefix") ?? ""
      const items = prefix === "/media/Series/" && query.get("kind") === "input"
        ? [{ value: "/media/Series/Episode 01.mkv", kind: "file" }]
        : [{ value: "/media/Series/", kind: "directory" }]
      await fulfillJson(route, { items, truncated: false })
    })
    const created = task(0, { input_path: "/media/Series/Episode 01.mkv", output_path: "/media/Series/Episode 01.AI.mp4", workflow: "anime-2x" })
    let body: unknown
    await page.route("**/api/tasks", async (route) => { body = route.request().postDataJSON(); await fulfillJson(route, created) })
    await page.route(`**/api/tasks/${created.id}?*`, async (route) => fulfillJson(route, taskDetail(created)))
    await page.goto(`${origin}/tasks`)
    expect(await page.evaluate(() => window.isSecureContext)).toBe(false)
    await page.getByRole("button", { name: "Add Task" }).click()
    const input = page.getByRole("combobox", { name: "Input Path", exact: true })
    await input.fill("/media/Se")
    await expect(page.getByRole("option", { name: "Series" })).toBeVisible()
    await input.press("ArrowDown")
    await input.press("Enter")
    await expect(input).toHaveValue("/media/Series/")
    await page.getByRole("option", { name: "Episode 01.mkv" }).click()
    await expect(input).toHaveValue(created.input_path)
    const output = page.getByRole("combobox", { name: "Output Path", exact: true })
    await output.fill("/media/Se")
    await page.getByRole("option", { name: "Series" }).click()
    await expect(output).toHaveValue("/media/Series/")
    await output.fill(created.output_path)
    const workflow = page.getByRole("combobox", { name: "Workflow", exact: true })
    await workflow.fill("anime")
    await expect(page.getByRole("option", { name: "anime-2x" })).toBeVisible()
    await page.screenshot({ path: `../.omo/evidence/controller-task-completion-${width}.png`, animations: "disabled" })
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width)
    const bounds = await page.getByRole("listbox").boundingBox()
    if (bounds === null) throw new Error("Suggestions have no visible bounds")
    expect(bounds.x).toBeGreaterThanOrEqual(0)
    expect(bounds.x + bounds.width).toBeLessThanOrEqual(width)
    expect((await new AxeBuilder({ page }).include(".task-dialog").analyze()).violations).toEqual([])
    await workflow.press("ArrowDown")
    await workflow.press("Enter")
    await expect(workflow).toHaveValue("anime-2x")
    await expect(page.getByRole("dialog")).toBeVisible()
    await page.getByRole("button", { name: "Create Task" }).click()
    await expect(page.getByRole("dialog")).toHaveCount(0)
    expect(body).toMatchObject({ input_path: created.input_path, output_path: created.output_path, workflow: created.workflow })
    expect(errors).toEqual([])
  })
}
