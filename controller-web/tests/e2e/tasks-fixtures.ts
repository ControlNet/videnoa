import { appendFile, mkdir } from "node:fs/promises"

import { expect, type Page, type Route } from "@playwright/test"

import type { FailureStage, Task, TaskAttempt, TaskDetail, TaskStatus } from "../../src/api/taskSchemas"

export const evidenceDir = "../.omo/evidence/videnoa-controller/task-19/playwright-report/screenshots/task-16"
export const statuses = [
  "queued", "reserved", "uploading", "staged", "submitting", "processing", "remote_completed",
  "downloading", "verifying", "publishing", "remote_cleanup", "completed", "failed", "cancelled",
] as const satisfies readonly TaskStatus[]

export type RequestJournal = {
  readonly tasks: string[]
  readonly counts: string[]
}

const session = {
  id: "550e8400-e29b-41d4-a716-446655440000",
  authenticated: true,
  method: "session",
  expires_at: "2030-01-01T00:00:00Z",
  idle_expires_at: "2030-01-01T00:00:00Z",
} as const

export function requestJournal(): RequestJournal {
  return { tasks: [], counts: [] }
}

export async function fulfillJson(route: Route, body: unknown, status = 200): Promise<void> {
  await route.fulfill({ status, contentType: "application/json", body: JSON.stringify(body) })
}

export async function installAuthenticatedSession(page: Page): Promise<void> {
  await page.route("**/api/auth/setup", async (route) => fulfillJson(route, { initialized: true }))
  await page.route("**/api/auth/session", async (route) => fulfillJson(route, session))
}

export function task(index: number, overrides: Partial<Task> = {}): Task {
  const status = statusFor(index)
  const completed = status === "completed" || status === "failed" || status === "cancelled"
  const base = {
    id: `00000000-0000-4000-8000-${String(index + 1).padStart(12, "0")}`,
    version: 1,
    status,
    input_path: inputPath(index),
    output_path: `/nas/output/episode-${String(index + 1).padStart(5, "0")}.mp4`,
    input_extension: "mkv",
    output_extension: "mp4",
    workflow: workflowFor(index),
    priority: 20_000 - index,
    source: sourceFor(index),
    source_reference: null,
    input_size: 4_294_967_296 + index,
    worker_id: workerFor(index),
    remote_job_id: status === "processing" ? "550e8400-e29b-41d4-a716-446655440099" : null,
    progress: {
      percent: completed ? 100 : index % 100,
      processed_frames: index * 100,
      total_frames: 10_000,
      frames_per_second: completed ? null : 23.8,
      eta_seconds: completed ? null : 420,
      bytes_transferred: null,
      bytes_total: null,
    },
    attempt_count: 1,
    failure: status === "failed" ? {
      failure_stage: "processing",
      failure_code: "processing_failed",
      message: "Worker reported a deliberately long diagnostic that must remain contained inside the task table without widening the document.",
      retryable: true,
    } : null,
    cancel_requested_at: null,
    created_at: new Date(Date.UTC(2030, 0, 1, 0, 0, index)).toISOString(),
    updated_at: new Date(Date.UTC(2030, 0, 1, 0, 5, index)).toISOString(),
    completed_at: completed ? new Date(Date.UTC(2030, 0, 1, 0, 5, index)).toISOString() : null,
  } as const satisfies Task
  return { ...base, ...overrides }
}

export function taskDetail(taskValue: Task, attempts: readonly TaskAttempt[] = []): TaskDetail {
  return { task: taskValue, attempts: [...attempts], total: attempts.length, limit: 100, offset: 0 }
}

export async function installPagedApi(page: Page, journal: RequestJournal, total = 20_000): Promise<void> {
  await installSession(page)
  await page.route("**/api/status-counts", async (route) => {
    journal.counts.push(route.request().url())
    await fulfillJson(route, countsFor(total))
  })
  await page.route("**/api/tasks?*", async (route) => {
    const url = new URL(route.request().url())
    journal.tasks.push(url.search)
    const limit = Number(url.searchParams.get("limit"))
    const offset = Number(url.searchParams.get("offset"))
    const matching = matchingIndices(url, total)
    const items = matching.slice(offset, offset + limit).map((index) => task(index))
    await fulfillJson(route, { items, total: matching.length, limit, offset })
  })
}

export async function installLiveApi(page: Page, journal: RequestJournal, initialTask: Task) {
  let currentTask = initialTask
  await page.addInitScript(() => {
    class TestEventSource extends EventTarget {
      static readonly CLOSED = 2
      readonly readyState = 1
      constructor() {
        super()
        Reflect.set(window, "testEventSource", this)
      }
      close(): void {}
    }
    Object.defineProperty(window, "EventSource", { value: TestEventSource })
  })
  await installAuthenticatedSession(page)
  await page.route("**/api/status-counts", async (route) => {
    journal.counts.push(route.request().url())
    await fulfillJson(route, {
      items: statuses.map((status) => ({ status, count: status === currentTask.status ? 1 : 0 })),
      total: 1,
    })
  })
  await page.route("**/api/tasks?*", async (route) => {
    const url = new URL(route.request().url())
    journal.tasks.push(url.search)
    const matches = matchesUrl(currentTask, url)
    await fulfillJson(route, { items: matches ? [currentTask] : [], total: matches ? 1 : 0, limit: 50, offset: 0 })
  })
  return { setTask: (nextTask: Task) => { currentTask = nextTask } }
}

export async function dispatchTaskUpdate(page: Page, taskUpdate: Task): Promise<void> {
  await page.evaluate((incoming) => {
    const events = Reflect.get(window, "testEventSource")
    if (events instanceof EventTarget) {
      events.dispatchEvent(new MessageEvent("task_updated", { data: JSON.stringify({
        type: "task_updated",
        data: { event_id: "550e8400-e29b-41d4-a716-446655440003", task: incoming },
      }) }))
    }
  }, taskUpdate)
}

export async function capture(page: Page, name: string): Promise<void> {
  expect(await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth)).toBe(false)
  await mkdir(`${evidenceDir}/tasks-table`, { recursive: true })
  await page.screenshot({ path: `${evidenceDir}/tasks-table/${name}`, animations: "disabled", fullPage: false, scale: "css" })
}

export async function appendEvidence(message: string): Promise<void> {
  await mkdir(evidenceDir, { recursive: true })
  await appendFile(`${evidenceDir}/tasks-errors.txt`, `${new Date().toISOString()} ${message}\n`, "utf8")
}

async function installSession(page: Page): Promise<void> {
  await page.addInitScript(() => Object.defineProperty(window, "EventSource", { value: undefined }))
  await installAuthenticatedSession(page)
}

function matchingIndices(url: URL, total: number): number[] {
  const indices: number[] = []
  for (let index = 0; index < total; index += 1) {
    if (matchesIndex(index, url)) indices.push(index)
  }
  const direction = url.searchParams.get("direction") === "asc" ? 1 : -1
  const sort = url.searchParams.get("sort") ?? "priority"
  indices.sort((left, right) => direction * compare(sortValue(left, sort), sortValue(right, sort)) || left - right)
  return indices
}

function matchesIndex(index: number, url: URL): boolean {
  const status = url.searchParams.get("status")
  if (status !== null && statusFor(index) !== status) return false
  const workflow = url.searchParams.get("workflow")
  if (workflow !== null && workflowFor(index) !== workflow) return false
  const worker = url.searchParams.get("worker_id")
  if (worker !== null && workerFor(index) !== worker) return false
  const source = url.searchParams.get("source")
  if (source !== null && sourceFor(index) !== source) return false
  const failureStage = url.searchParams.get("failure_stage")
  if (failureStage !== null && failureStageFor(index) !== failureStage) return false
  const search = url.searchParams.get("search")?.toLocaleLowerCase() ?? ""
  return search === "" || inputPath(index).toLocaleLowerCase().includes(search)
}

function matchesUrl(candidate: Task, url: URL): boolean {
  const status = url.searchParams.get("status")
  const workflow = url.searchParams.get("workflow")
  const worker = url.searchParams.get("worker_id")
  const source = url.searchParams.get("source")
  const failureStage = url.searchParams.get("failure_stage")
  const search = url.searchParams.get("search")?.toLocaleLowerCase() ?? ""
  return (status === null || candidate.status === status)
    && (workflow === null || candidate.workflow === workflow)
    && (worker === null || candidate.worker_id === worker)
    && (source === null || candidate.source === source)
    && (failureStage === null || candidate.failure?.failure_stage === failureStage)
    && (search === "" || candidate.input_path.toLocaleLowerCase().includes(search) || candidate.output_path.toLocaleLowerCase().includes(search))
}

function sortValue(index: number, sort: string): number | string {
  switch (sort) {
    case "priority": return 20_000 - index
    case "created_at": return index
    case "completed_at": return isCompleted(statusFor(index)) ? index : -1
    case "status": return statusFor(index)
    case "worker": return workerFor(index)
    case "duration": return isCompleted(statusFor(index)) ? 300 : index % 300
    default: return 20_000 - index
  }
}

function compare(left: number | string, right: number | string): number {
  return left < right ? -1 : left > right ? 1 : 0
}

function countsFor(total: number) {
  return {
    items: statuses.map((status, statusIndex) => ({
      status,
      count: Math.floor(total / statuses.length) + (statusIndex < total % statuses.length ? 1 : 0),
    })),
    total,
  }
}

function statusFor(index: number): TaskStatus {
  return statuses[index % statuses.length] ?? "queued"
}

function workflowFor(index: number): string {
  return index % 2 === 0 ? "anime-2x" : "rife-2x"
}

function workerFor(index: number): string {
  return `550e8400-e29b-41d4-a716-${String(index % 3 + 1).padStart(12, "0")}`
}

function sourceFor(index: number): Task["source"] {
  return index % 2 === 0 ? "manual" : "api"
}

function failureStageFor(index: number): FailureStage | null {
  return statusFor(index) === "failed" ? "processing" : null
}

function inputPath(index: number): string {
  return `/nas/library/anime/season-very-long-name/episode-${String(index + 1).padStart(5, "0")}.mkv`
}

function isCompleted(status: TaskStatus): boolean {
  return status === "completed" || status === "failed" || status === "cancelled"
}
