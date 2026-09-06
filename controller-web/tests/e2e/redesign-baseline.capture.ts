import { mkdir } from "node:fs/promises"

import { expect, type Page, test } from "@playwright/test"

import { installOperationalReadRoutes } from "./operations-fixtures"
import { fulfillJson, installAuthenticatedSession, statuses, task, taskDetail } from "./tasks-fixtures"

// Captures the pre-redesign visual baseline with populated operational data.
// Not part of the default suite: the filename is deliberately outside Playwright's
// spec/test match so `npm run test:e2e` stays unchanged.

const outputDir = process.env["BASELINE_OUT"] ?? "/tmp/baseline"

const viewports = {
  desktop: { width: 1440, height: 900 },
  tablet: { width: 768, height: 900 },
  narrow: { width: 375, height: 812 },
} as const

const rows = Array.from({ length: 24 }, (_, index) => task(index))
const detailTask = task(0)

const unauthenticatedSession = {
  error: "unauthorized",
} as const

async function capture(page: Page, name: string, scheme: "dark" | "light" = "dark"): Promise<void> {
  await page.emulateMedia({ colorScheme: scheme, reducedMotion: "reduce" })
  await page.evaluate(async () => {
    await Promise.all(document.getAnimations().map(async (animation) => animation.finished.catch(() => undefined)))
  })
  await mkdir(outputDir, { recursive: true })
  await page.screenshot({ path: `${outputDir}/${name}.png`, animations: "disabled", fullPage: false, scale: "css" })
}

async function captureAcrossViewports(page: Page, name: string): Promise<void> {
  for (const [viewportName, viewport] of Object.entries(viewports)) {
    await page.setViewportSize(viewport)
    await capture(page, `${name}-${viewportName}`, "dark")
  }
  await page.setViewportSize(viewports.desktop)
  await capture(page, `${name}-desktop-light`, "light")
}

async function installTaskRoutes(page: Page): Promise<void> {
  await page.route("**/api/events", async (route) => {
    await route.fulfill({ status: 200, contentType: "text/event-stream", body: "" })
  })
  await installAuthenticatedSession(page)
  await page.route("**/api/status-counts", async (route) =>
    fulfillJson(route, {
      items: statuses.map((status) => ({
        status,
        count: rows.filter((row) => row.status === status).length,
      })),
      total: rows.length,
    }),
  )
  await page.route("**/api/tasks?*", async (route) => {
    const url = new URL(route.request().url())
    const limit = Number(url.searchParams.get("limit") ?? 50)
    const offset = Number(url.searchParams.get("offset") ?? 0)
    await fulfillJson(route, { items: rows.slice(offset, offset + limit), total: rows.length, limit, offset })
  })
  await page.route(`**/api/tasks/${detailTask.id}?*`, async (route) =>
    fulfillJson(
      route,
      taskDetail(detailTask, [
        {
          id: "00000000-0000-4000-8000-000000000002",
          task_id: detailTask.id,
          attempt_number: 1,
          worker_id: detailTask.worker_id,
          status: detailTask.status,
          submission_key: "00000000-0000-4000-8000-000000000004",
          remote_job_id: detailTask.remote_job_id,
          remote_input_path: "task/input/opaque.mkv",
          remote_output_path: "task/output/opaque.mp4",
          progress: detailTask.progress,
          retry: { retry_count: 0, next_retry_at: null },
          failure: null,
          created_at: detailTask.created_at,
          started_at: detailTask.updated_at,
          completed_at: null,
        },
      ]),
    ),
  )
  await installOperationalReadRoutes(page)
}

test("captures the unauthenticated sign-in baseline", async ({ page }) => {
  await page.route("**/api/auth/setup", async (route) => fulfillJson(route, { initialized: true }))
  await page.route("**/api/auth/session", async (route) => fulfillJson(route, unauthenticatedSession, 401))
  await page.goto("/")
  await expect(page.getByRole("heading", { name: "Sign in to Controller" })).toBeVisible()
  await captureAcrossViewports(page, "login")
})

test("captures the authenticated operational baseline", async ({ page }) => {
  await installTaskRoutes(page)

  await page.goto("/tasks")
  await expect(page.getByRole("button", { name: /Open task/ }).first()).toBeVisible()
  await captureAcrossViewports(page, "tasks")

  await page.setViewportSize(viewports.desktop)
  await page.getByRole("button", { name: /Open task/ }).first().click()
  await expect(page.getByRole("button", { name: "Close task detail" })).toBeVisible()
  await captureAcrossViewports(page, "tasks-detail")

  await page.setViewportSize(viewports.desktop)
  await page.getByRole("button", { name: "Close task detail" }).click()
  await page.getByRole("button", { name: "Add Task" }).click()
  await expect(page.getByRole("dialog")).toBeVisible()
  await captureAcrossViewports(page, "tasks-dialog")
  await page.keyboard.press("Escape")

  await page.goto("/workers")
  await expect(page.getByRole("button", { name: "Add Worker" })).toBeVisible()
  await captureAcrossViewports(page, "workers")

  await page.goto("/settings")
  await expect(page.getByRole("heading", { name: "Settings" })).toBeVisible()
  await captureAcrossViewports(page, "settings")
})
