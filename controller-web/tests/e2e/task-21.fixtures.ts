import { mkdir, rm } from "node:fs/promises"

import { expect, type Locator, type Page } from "@playwright/test"

import type { Task, TaskAttempt, TaskDetail } from "../../src/api/taskSchemas"

export const task21EvidenceDir = "../.omo/evidence/videnoa-controller/task-21/screenshots"

export type Gate = {
  readonly wait: Promise<void>
  readonly release: () => void
}

export function createGate(): Gate {
  const releases: Array<() => void> = []
  const wait = new Promise<void>((resolve) => releases.push(resolve))
  return {
    wait,
    release: () => {
      const release = releases.shift()
      if (release === undefined) throw new RangeError("gate has no pending release")
      release()
    },
  }
}

export function detailPage(taskValue: Task, attempts: readonly TaskAttempt[], total: number, offset = 0): TaskDetail {
  return { task: taskValue, attempts: [...attempts], total, limit: 100, offset }
}

export function attempt(taskValue: Task, attemptNumber: number, overrides: Partial<TaskAttempt> = {}): TaskAttempt {
  const suffix = `${taskValue.id.slice(-6)}${String(attemptNumber).padStart(6, "0")}`
  const base = {
    id: `10000000-0000-4000-8000-${suffix}`,
    task_id: taskValue.id,
    attempt_number: attemptNumber,
    worker_id: taskValue.worker_id,
    status: taskValue.status,
    submission_key: `20000000-0000-4000-8000-${suffix}`,
    remote_job_id: taskValue.remote_job_id,
    remote_input_path: null,
    remote_output_path: null,
    progress: taskValue.progress,
    retry: { retry_count: 0, next_retry_at: null },
    failure: null,
    created_at: taskValue.created_at,
    started_at: taskValue.created_at,
    completed_at: taskValue.completed_at,
  } satisfies TaskAttempt
  return { ...base, ...overrides }
}

export async function installAbortResistantHistoryFetch(page: Page): Promise<void> {
  await page.addInitScript(() => {
    const nativeFetch = globalThis.fetch
    globalThis.fetch = (input, init) => {
      const request = new Request(input, init)
      const url = new URL(request.url)
      const isHistoryPage = url.pathname.startsWith("/api/tasks/") && url.searchParams.get("offset") !== "0"
      if (!isHistoryPage) return Reflect.apply(nativeFetch, globalThis, [input, init])
      const abortResistantRequest = new Request(request.url, {
        credentials: request.credentials,
        headers: request.headers,
        method: request.method,
      })
      return Reflect.apply(nativeFetch, globalThis, [abortResistantRequest])
    }
  })
}

export async function captureTask21(page: Page, filename: string, target?: Locator): Promise<void> {
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth)).toBe(true)
  await mkdir(task21EvidenceDir, { recursive: true })
  const path = `${task21EvidenceDir}/${filename}`
  if (target === undefined) {
    await page.screenshot({ path, animations: "disabled", fullPage: false, scale: "css" })
    return
  }
  await target.screenshot({ path, animations: "disabled", scale: "css" })
}

export async function resetTask21Screenshots(): Promise<void> {
  await rm(task21EvidenceDir, { recursive: true, force: true })
}

export async function alignDetailLabelBelowHeader(page: Page, label: string, fromEnd: boolean): Promise<void> {
  await page.getByRole("region", { name: "Task Detail" }).evaluate((pane, options) => {
    const header = pane.querySelector(".task-detail-header")
    const labels = Array.from(pane.querySelectorAll("dt")).filter((element) => element.textContent === options.label)
    const target = options.fromEnd ? labels.at(-1) : labels.at(0)
    if (!(header instanceof HTMLElement) || !(target instanceof HTMLElement)) throw new TypeError("detail evidence target is missing")
    pane.scrollTop += target.getBoundingClientRect().top - header.getBoundingClientRect().bottom - 12
  }, { label, fromEnd })
}

export async function alignDetailPaneInViewport(page: Page): Promise<void> {
  await page.getByRole("region", { name: "Task Detail" }).evaluate((pane) => {
    const scrollOwner = pane.closest(".shell-main")
    if (!(scrollOwner instanceof HTMLElement)) throw new TypeError("shell scroll owner is missing")
    scrollOwner.scrollTop += pane.getBoundingClientRect().top - 16
  })
}

export async function alignDetailPaneToEnd(page: Page): Promise<void> {
  await page.getByRole("region", { name: "Task Detail" }).evaluate((pane) => {
    pane.scrollTop = Math.max(0, pane.scrollHeight - pane.clientHeight - 4)
  })
}

export async function expectDetailEndReachable(page: Page): Promise<void> {
  expect(await page.getByRole("region", { name: "Task Detail" }).evaluate((pane) => {
    const header = pane.querySelector(".task-detail-header")
    const errorSection = pane.querySelector(".task-detail-content > section:last-child")
    const finalContent = errorSection?.lastElementChild
    if (!(header instanceof HTMLElement) || !(finalContent instanceof HTMLElement)) {
      throw new TypeError("detail end evidence target is missing")
    }
    const paneRect = pane.getBoundingClientRect()
    const headerRect = header.getBoundingClientRect()
    const finalRect = finalContent.getBoundingClientRect()
    const remainingScroll = pane.scrollHeight - pane.clientHeight - pane.scrollTop
    return {
      withinFinalScrollRange: remainingScroll >= 0 && remainingScroll <= 4,
      hasSafeBottomPadding: paneRect.bottom - finalRect.bottom >= 12,
      finalContentBelowHeader: finalRect.top >= headerRect.bottom,
      finalContentFullyVisible: finalRect.bottom <= paneRect.bottom,
      ownsVerticalOverflow: pane.scrollHeight > pane.clientHeight && getComputedStyle(pane).overflowY === "auto",
    }
  })).toEqual({
    withinFinalScrollRange: true,
    hasSafeBottomPadding: true,
    finalContentBelowHeader: true,
    finalContentFullyVisible: true,
    ownsVerticalOverflow: true,
  })
}
