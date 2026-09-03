import { expect, test } from "@playwright/test"

import { appendEvidence, capture, dispatchTaskUpdate, installLiveApi, installPagedApi, requestJournal, task } from "./tasks-fixtures"

test("keeps 20,000 task history bounded through filters, sorting, paging, and narrow table navigation", async ({ page }) => {
  // Given: a deterministic 20,000-task server that creates only requested-page rows.
  const journal = requestJournal()
  await installPagedApi(page, journal)
  await page.setViewportSize({ width: 1280, height: 900 })
  await page.emulateMedia({ colorScheme: "dark", reducedMotion: "reduce" })
  await page.goto("/tasks?columns=path,error,remote_job")
  await expect(page.getByRole("table").locator("tbody tr")).toHaveCount(50)
  expect(journal.tasks).toHaveLength(1)
  expect(journal.counts).toHaveLength(1)

  // When: the operator applies coherent workflow/search state.
  await page.getByRole("group", { name: "Task filters" }).getByLabel("Workflow").fill("anime-2x")
  await expect.poll(() => journal.tasks.length).toBe(2)
  await expect.poll(() => journal.counts.length).toBe(2)
  await page.getByLabel("Search task paths").fill("episode-00101")
  await expect(page).toHaveURL(/search=episode-00101/)
  await expect.poll(() => journal.tasks.length).toBe(3)
  await expect.poll(() => journal.counts.length).toBe(3)

  // Then: the visible row matches the active filters and desktop evidence.
  const filteredRows = page.getByRole("table").locator("tbody tr")
  await expect(filteredRows).toHaveCount(1)
  await expect(filteredRows.first()).toContainText("episode-00101.mkv")
  await expect(filteredRows.first()).toContainText("anime-2x")
  await expect(page.getByLabel("Search task paths")).toHaveAttribute("name", "search")
  await expect(page.getByRole("group", { name: "Task filters" }).getByLabel("Workflow")).toHaveAttribute("autocomplete", "off")
  await capture(page, "desktop.png")

  // When: the operator clears search, changes ordering, and requests the next bounded page.
  await page.getByLabel("Search task paths").fill("")
  await expect.poll(() => journal.tasks.length).toBe(4)
  await expect.poll(() => journal.counts.length).toBe(4)
  await page.getByLabel("Sort").selectOption("created_at")
  await expect.poll(() => journal.tasks.length).toBe(5)
  await expect.poll(() => journal.counts.length).toBe(5)
  await page.getByRole("button", { name: "Next" }).click()
  await expect(page).toHaveURL(/offset=50/)
  await expect.poll(() => journal.tasks.length).toBe(6)
  await expect.poll(() => journal.counts.length).toBe(6)
  await expect(page.getByRole("table").locator("tbody tr")).toHaveCount(50)

  // Then: task and count requests remain independently one-per-trigger and bounded.
  expect(journal.tasks).toHaveLength(6)
  expect(journal.counts).toHaveLength(6)
  for (const request of journal.tasks) {
    expect(Number(new URLSearchParams(request).get("limit"))).toBeLessThanOrEqual(100)
  }
  await page.setViewportSize({ width: 1280, height: 1080 })
  await expect(page.locator(".task-pagination")).toBeInViewport()
  await capture(page, "desktop-pagination.png")

  // When: narrow evidence intentionally navigates both shell and table scroll owners.
  await page.setViewportSize({ width: 375, height: 812 })
  await page.locator(".shell-main").evaluate((element) => {
    element.scrollTop = 430
  })
  await page.locator(".task-table-frame").evaluate((element) => {
    element.scrollTop = 132
    element.scrollLeft = 420
  })

  // Then: body rows and horizontal table navigation remain visible without page overflow.
  expect(await page.locator(".task-table-frame").evaluate((element) => element.scrollLeft)).toBeGreaterThan(0)
  expect(await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth)).toBe(false)
  await capture(page, "narrow.png")
  await page.locator(".task-table-frame").evaluate((element) => {
    element.scrollTop = element.scrollHeight
  })
  await page.locator(".shell-main").evaluate((element) => {
    element.scrollTop = element.scrollHeight
  })
  expect(await page.locator(".task-table-frame").evaluate((element) => element.scrollTop)).toBeGreaterThan(132)
  await capture(page, "narrow-pagination.png")
})

test("ignores stale and equal SSE task versions without mutation or refetch", async ({ page }) => {
  // Given: one active row and independent request journals.
  const journal = requestJournal()
  const current = task(0, { version: 3 })
  await installLiveApi(page, journal, current)
  await page.goto("/tasks")
  const row = page.getByRole("table").locator("tbody tr").first()
  await expect(row).toContainText("0%")

  // When: equal and stale versions arrive.
  await dispatchTaskUpdate(page, {
    ...current,
    progress: { ...current.progress, percent: 80 },
  })
  await dispatchTaskUpdate(page, {
    ...current,
    version: 2,
    progress: { ...current.progress, percent: 90 },
  })

  // Then: neither the row nor either request stream changes.
  await expect(row).toContainText("0%")
  await expect.poll(() => journal.tasks.length).toBe(1)
  await expect.poll(() => journal.counts.length).toBe(1)
})

test("merges a same-status same-order active progress delta without refetch", async ({ page }) => {
  // Given: one active row ordered by stable priority.
  const journal = requestJournal()
  const current = task(0)
  await installLiveApi(page, journal, current)
  await page.goto("/tasks")
  await expect(page.getByRole("table").locator("tbody tr")).toHaveCount(1)

  // When: a newer update changes only progress-bearing fields.
  await dispatchTaskUpdate(page, {
    ...current,
    version: 2,
    progress: { ...current.progress, percent: 42 },
  })

  // Then: one row changes and neither bounded endpoint refetches.
  await expect(page.getByRole("table").locator("tbody tr").first()).toContainText("42%")
  expect(journal.tasks).toHaveLength(1)
  expect(journal.counts).toHaveLength(1)
})

test("does not replay a retained task update when the Tasks route remounts", async ({ page }) => {
  // Given: a mounted Tasks page that has locally merged one valid live update.
  const journal = requestJournal()
  const current = task(0)
  await installLiveApi(page, journal, current)
  await page.goto("/tasks")
  await expect(page.getByRole("table").locator("tbody tr")).toHaveCount(1)
  await dispatchTaskUpdate(page, {
    ...current,
    version: 2,
    progress: { ...current.progress, percent: 42 },
  })
  await expect(page.getByRole("table").locator("tbody tr").first()).toContainText("42%")
  expect(journal.tasks).toHaveLength(1)
  expect(journal.counts).toHaveLength(1)

  // When: the operator leaves Tasks and returns while the global update snapshot is retained.
  await page.getByRole("link", { name: "Workers" }).click()
  await expect(page.getByRole("heading", { name: "Workers" })).toBeVisible()
  await page.getByRole("link", { name: "Tasks" }).click()
  await expect(page.getByRole("table").locator("tbody tr")).toHaveCount(1)

  // Then: remount performs exactly one fresh request per bounded endpoint.
  await expect.poll(() => journal.tasks.length).toBe(2)
  await expect.poll(() => journal.counts.length).toBe(2)
})

test("refetches one page and one count set when SSE changes active status membership", async ({ page }) => {
  // Given: a queued row in the current all-status page.
  const journal = requestJournal()
  const current = task(0)
  const liveApi = await installLiveApi(page, journal, current)
  await page.goto("/tasks")
  await expect(page.getByRole("table").locator("tbody tr").first()).toContainText("Queued")
  await expect.poll(() => journal.tasks.length).toBe(1)
  await expect.poll(() => journal.counts.length).toBe(1)

  // When: the row transitions to another active status.
  const processing = { ...current, version: 2, status: "processing" } as const
  liveApi.setTask(processing)
  await dispatchTaskUpdate(page, processing)

  // Then: membership is recovered with exactly one bounded page and count refetch.
  await expect.poll(() => journal.tasks.length).toBe(2)
  await expect.poll(() => journal.counts.length).toBe(2)
  await expect(page.getByRole("table").locator("tbody tr").first()).toContainText("Processing")
})

test("refetches one page and one count set when SSE changes the sorted field", async ({ page }) => {
  // Given: a priority-sorted active row.
  const journal = requestJournal()
  const current = task(0)
  const liveApi = await installLiveApi(page, journal, current)
  await page.goto("/tasks?sort=priority")
  await expect(page.getByRole("table").locator("tbody tr")).toHaveCount(1)
  await expect.poll(() => journal.tasks.length).toBe(1)
  await expect.poll(() => journal.counts.length).toBe(1)

  // When: a newer delta changes priority.
  const reordered = { ...current, version: 2, priority: current.priority + 1 }
  liveApi.setTask(reordered)
  await dispatchTaskUpdate(page, reordered)

  // Then: ordering is recovered with exactly one bounded request per endpoint.
  await expect.poll(() => journal.tasks.length).toBe(2)
  await expect.poll(() => journal.counts.length).toBe(2)
})

test("corrects a deep empty page directly to the canonical last valid page", async ({ page }) => {
  // Given: live shrinkage leaves a direct URL far beyond a 123-row result set.
  const journal = requestJournal()
  await installPagedApi(page, journal, 123)

  // When: the deep page responds empty with coherent server metadata.
  await page.goto("/tasks?limit=50&offset=10000")

  // Then: one correction reaches offset 100 without intermediate page requests.
  await expect(page).toHaveURL(/offset=100(?:&|$)/)
  await expect(page.getByRole("table").locator("tbody tr")).toHaveCount(23)
  expect(journal.tasks.map((request) => Number(new URLSearchParams(request).get("offset")))).toEqual([10_000, 100])
  expect(journal.counts).toHaveLength(2)
  await appendEvidence("empty-page recovery: offset 10000 corrected once to canonical offset 100; task requests=2; count requests=2")
})
