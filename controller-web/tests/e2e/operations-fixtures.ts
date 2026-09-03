import type { Page, Route } from "@playwright/test"

import { type SettingsResponse, settingsUpdateRequestSchema } from "../../src/api/settingsSchemas"
import { type Worker, workerCreateRequestSchema, workerUpdateRequestSchema } from "../../src/api/workerSchemas"

const session = {
  id: "550e8400-e29b-41d4-a716-446655440000",
  authenticated: true,
  method: "session",
  expires_at: "2030-01-01T00:00:00Z",
  idle_expires_at: "2030-01-01T00:00:00Z",
} as const

const workerTemplate: Worker = {
  id: "d2719a65-16d5-4e97-a756-d8f782769144",
  version: 4,
  name: "render-east",
  api_url: "http://worker-east.test:3000",
  enabled: true,
  online: false,
  compute_slots: 4,
  capabilities: { workflows: [{ name: "anime-2x", kind: "workflow" }], refreshed_at: "2026-09-03T10:00:00Z" },
  capacity: {
    used_slots: 2,
    available_slots: 2,
    assigned_tasks: 3,
    staged_tasks: 1,
    processing_tasks: 2,
    active_uploads: 1,
    active_downloads: 0,
    progress: null,
  },
  last_seen_at: "2026-09-03T10:02:00Z",
  last_assigned_at: "2026-09-03T10:01:00Z",
  created_at: "2026-09-01T08:00:00Z",
  updated_at: "2026-09-03T10:02:00Z",
  last_error: "health probe timed out",
}

const settingsTemplate: SettingsResponse = {
  version: 7,
  paths: {
    input_roots: ["/media/input"],
    output_roots: ["/media/output"],
    data_root: "/var/lib/videnoa",
    temp_root: "/var/tmp/videnoa",
    password_hash_file: "/run/secrets/controller-password",
  },
  secure_cookie: true,
  session_absolute_seconds: 86_400,
  session_idle_seconds: 3_600,
  scheduler: {
    paused: false,
    default_compute_slots: 4,
    prefetch_per_worker: 2,
    max_concurrent_uploads: 2,
    max_concurrent_downloads: 3,
  },
  timeouts: { health_seconds: 15, poll_seconds: 5, transfer_seconds: 300 },
  retry: { initial_seconds: 2, maximum_seconds: 30, max_attempts: 4 },
}

const readiness = {
  status: "ready",
  checks: [
    { name: "persistence", ready: true, message: null },
    { name: "scheduler", ready: true, message: null },
  ],
} as const

async function json(route: Route, body: unknown, status = 200, headers: Record<string, string> = {}): Promise<void> {
  await route.fulfill({ status, contentType: "application/json", headers, body: JSON.stringify(body) })
}

export type OperationalApi = {
  readonly journal: readonly string[]
  readonly allowNextWorkerDelete: () => void
  readonly staleNextSettingsSave: () => void
  readonly staleNextWorkerUpdate: () => void
  readonly unauthenticateNextMutation: () => void
}

export async function installOperationalReadRoutes(page: Page): Promise<void> {
  await page.route("**/api/workers", async (route) => json(route, { items: [workerTemplate], total: 1 }))
  await page.route("**/api/settings", async (route) => json(route, settingsTemplate))
  await page.route("**/api/readiness", async (route) => json(route, readiness))
}

export async function installOperationalApi(page: Page): Promise<OperationalApi> {
  const journal: string[] = []
  let workers = [workerTemplate]
  let settings = settingsTemplate
  let settingsSaveIsStale = false
  let workerUpdateIsStale = false
  let mutationIsUnauthorized = false
  let workerDeleteIsAllowed = false

  await page.route("**/api/events", async (route) => {
    await route.fulfill({ status: 200, contentType: "text/event-stream", body: "" })
  })
  await page.route("**/api/auth/session", async (route) => json(route, session, 200, { "x-csrf-token": "session-proof" }))
  await page.route("**/api/readiness", async (route) => json(route, readiness))
  await page.route("**/api/workers", async (route) => {
    if (route.request().method() === "GET") {
      journal.push("workers:get")
      await json(route, { items: workers, total: workers.length })
      return
    }
    const request = workerCreateRequestSchema.parse(await route.request().postDataJSON())
    journal.push(`create:${request.name}:${request.compute_slots}`)
    if (workers.some((worker) => worker.name === request.name)) {
      await json(route, { error: { code: "conflict", message: "worker name is already registered", retryable: false, field_errors: [] } }, 409)
      return
    }
    if (workers.some((worker) => worker.api_url === request.api_url)) {
      await json(route, { error: { code: "conflict", message: "worker API URL is already registered", retryable: false, field_errors: [] } }, 409)
      return
    }
    const created = { ...workerTemplate, id: "7a43d6b5-4396-4d48-9d3c-f84885bf42a8", version: 1, ...request, api_url: request.api_url, online: false, capacity: { ...workerTemplate.capacity, used_slots: 0, available_slots: request.compute_slots, assigned_tasks: 0, staged_tasks: 0, processing_tasks: 0, active_uploads: 0 }, last_seen_at: null, last_assigned_at: null, last_error: null }
    workers = [...workers, created]
    await json(route, created, 201)
  })
  await page.route("**/api/workers/**", async (route) => {
    const url = new URL(route.request().url())
    const id = url.pathname.split("/")[3] ?? ""
    if (mutationIsUnauthorized) {
      mutationIsUnauthorized = false
      journal.push(`unauthorized:${route.request().method()}:${url.pathname}`)
      await json(route, { error: "unauthorized" }, 401)
      return
    }
    const worker = workers.find((candidate) => candidate.id === id)
    if (worker === undefined) {
      await json(route, { error: { code: "not_found", message: "worker was not found", retryable: false, field_errors: [] } }, 404)
      return
    }
    if (route.request().method() === "DELETE") {
      journal.push(`delete:${id}:v${url.searchParams.get("version") ?? "missing"}`)
      if (workerDeleteIsAllowed) {
        workerDeleteIsAllowed = false
        workers = workers.filter((candidate) => candidate.id !== id)
        await json(route, { worker_id: id, deleted: true })
        return
      }
      await json(route, { error: { code: "conflict", message: "worker is referenced by tasks", retryable: false, field_errors: [] } }, 409)
      return
    }
    if (url.pathname.endsWith("/enable") || url.pathname.endsWith("/disable")) {
      const enabled = url.pathname.endsWith("/enable")
      journal.push(`${enabled ? "enable" : "disable"}:${id}`)
      const updated = { ...worker, version: worker.version + 1, enabled }
      workers = workers.map((candidate) => candidate.id === id ? updated : candidate)
      await json(route, updated)
      return
    }
    const request = workerUpdateRequestSchema.parse(await route.request().postDataJSON())
    journal.push(`update:${id}:v${request.version}:${request.compute_slots}`)
    if (workerUpdateIsStale) {
      workerUpdateIsStale = false
      const external = { ...worker, version: worker.version + 1 }
      workers = workers.map((candidate) => candidate.id === id ? external : candidate)
      await json(route, { error: { code: "conflict", message: "worker changed since it was read", retryable: false, field_errors: [] } }, 409)
      return
    }
    if (request.version !== worker.version) {
      await json(route, { error: { code: "conflict", message: "worker changed since it was read", retryable: false, field_errors: [] } }, 409)
      return
    }
    if (request.compute_slots < worker.capacity.used_slots) {
      await json(route, { error: { code: "conflict", message: "worker capacity is below durable usage", retryable: false, field_errors: [] } }, 409)
      return
    }
    const updated = { ...worker, ...request, version: worker.version + 1 }
    workers = workers.map((candidate) => candidate.id === id ? updated : candidate)
    await json(route, updated)
  })
  await page.route("**/api/settings", async (route) => {
    if (route.request().method() === "GET") {
      journal.push("settings:get")
      await json(route, settings)
      return
    }
    const request = settingsUpdateRequestSchema.parse(await route.request().postDataJSON())
    journal.push(`settings:v${request.version}:uploads${request.scheduler.max_concurrent_uploads}`)
    if (settingsSaveIsStale) {
      settingsSaveIsStale = false
      settings = { ...settings, version: settings.version + 1 }
      await json(route, { error: { code: "conflict", message: "settings changed since they were read", retryable: false, field_errors: [] } }, 409)
      return
    }
    if (request.version !== settings.version) {
      await json(route, { error: { code: "conflict", message: "settings changed since they were read", retryable: false, field_errors: [] } }, 409)
      return
    }
    settings = { ...settings, ...request, version: settings.version + 1 }
    await json(route, settings)
  })
  await page.route("**/api/scheduler/**", async (route) => {
    const paused = new URL(route.request().url()).pathname.endsWith("/pause")
    journal.push(paused ? "pause" : "resume")
    settings = { ...settings, version: settings.version + 1, scheduler: { ...settings.scheduler, paused } }
    await json(route, settings)
  })
  return {
    journal,
    allowNextWorkerDelete: () => { workerDeleteIsAllowed = true },
    staleNextSettingsSave: () => { settingsSaveIsStale = true },
    staleNextWorkerUpdate: () => { workerUpdateIsStale = true },
    unauthenticateNextMutation: () => { mutationIsUnauthorized = true },
  }
}
