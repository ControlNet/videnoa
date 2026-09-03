import { describe, expect, it } from "vitest"

import {
  workerCreateRequestSchema,
  workerDeleteResponseSchema,
  workerListSchema,
  workerUpdateRequestSchema,
} from "./workerSchemas"

const worker = {
  id: "550e8400-e29b-41d4-a716-446655440000",
  version: 4,
  name: "render-east",
  api_url: "https://worker.example/api/",
  enabled: true,
  online: false,
  compute_slots: 4,
  capabilities: {
    workflows: [{ name: "anime-2x", kind: "workflow" }],
    refreshed_at: "2030-01-01T00:00:00Z",
  },
  capacity: {
    used_slots: 2,
    available_slots: 2,
    assigned_tasks: 3,
    staged_tasks: 1,
    processing_tasks: 2,
    active_uploads: 1,
    active_downloads: 0,
    progress: {
      percent: 45,
      processed_frames: 450,
      total_frames: 1000,
      frames_per_second: 24,
      eta_seconds: 23,
      bytes_transferred: null,
      bytes_total: null,
    },
  },
  last_seen_at: "2030-01-01T00:01:00Z",
  last_assigned_at: "2030-01-01T00:00:30Z",
  created_at: "2030-01-01T00:00:00Z",
  updated_at: "2030-01-01T00:01:00Z",
  last_error: "health check failed",
} as const

describe("worker API schemas", () => {
  it("parses the exact Rust worker list payload", () => {
    // Given: a complete worker summary returned by GET /api/workers.
    const payload = { items: [worker], total: 1 }

    // When: the payload crosses the frontend boundary.
    const parsed = workerListSchema.safeParse(payload)

    // Then: every operational field remains available to the Workers table.
    expect(parsed.success).toBe(true)
  })

  it("rejects unknown worker summary fields", () => {
    // Given: a server response that drifted beyond the closed Rust DTO.
    const payload = { items: [{ ...worker, secret: "must-not-cross" }], total: 1 }

    // When: the strict boundary parses it.
    const parsed = workerListSchema.safeParse(payload)

    // Then: contract drift cannot silently reach the UI.
    expect(parsed.success).toBe(false)
  })

  it.each([
    ["credentials", "https://user:pass@worker.example/api"],
    ["query", "https://worker.example/api?token=value"],
    ["fragment", "https://worker.example/api#status"],
    ["scheme", "ftp://worker.example/api"],
  ])("rejects a worker URL containing unsupported %s", (_case, apiUrl) => {
    // Given: an API base URL rejected by WorkerApiUrl.
    const payload = { name: "render-east", api_url: apiUrl, enabled: true, compute_slots: 2 }

    // When: the create request is checked before submission.
    const parsed = workerCreateRequestSchema.safeParse(payload)

    // Then: the client enforces the same URL policy as Rust.
    expect(parsed.success).toBe(false)
  })

  it.each([0, 65_536])("rejects compute slot bound %s", (computeSlots) => {
    // Given: a slot count outside NonZeroU16.
    const payload = { name: "render-east", api_url: "https://worker.example/api", enabled: true, compute_slots: computeSlots }

    // When: the request boundary parses it.
    const parsed = workerCreateRequestSchema.safeParse(payload)

    // Then: invalid capacity never reaches the mutation endpoint.
    expect(parsed.success).toBe(false)
  })

  it("preserves update versions and accepts the maximum slot count", () => {
    // Given: an exact optimistic update at the server maximum.
    const payload = { version: 7, name: "render-east", api_url: "http://worker.local:3000/api", enabled: false, compute_slots: 65_535 }

    // When: the update request is parsed.
    const parsed = workerUpdateRequestSchema.safeParse(payload)

    // Then: the version and spelling are preserved without client normalization.
    expect(parsed.success && parsed.data).toEqual(payload)
  })

  it("parses the exact delete acknowledgement", () => {
    // Given: the Rust delete response.
    const payload = { worker_id: worker.id, deleted: true }

    // When/Then: the closed response is accepted.
    expect(workerDeleteResponseSchema.safeParse(payload).success).toBe(true)
  })
})
