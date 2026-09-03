import { describe, expect, it } from "vitest"

import {
  readinessSchema,
  type SettingsUpdateRequest,
  settingsResponseSchema,
  settingsUpdateRequestSchema,
} from "./settingsSchemas"

const settings = {
  version: 3,
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
    default_compute_slots: 2,
    prefetch_per_worker: 1,
    max_concurrent_uploads: 2,
    max_concurrent_downloads: 3,
  },
  timeouts: {
    health_seconds: 15,
    poll_seconds: 5,
    transfer_seconds: 300,
  },
  retry: {
    initial_seconds: 2,
    maximum_seconds: 30,
    max_attempts: 4,
  },
} as const

describe("settings API schemas", () => {
  it("parses mutable runtime settings and read-only restart configuration", () => {
    // Given: the exact GET /api/settings response.
    // When: it crosses the frontend boundary.
    const parsed = settingsResponseSchema.safeParse(settings)

    // Then: runtime and restart-required values remain structurally distinct.
    expect(parsed.success).toBe(true)
  })

  it("accepts exact scheduler, timeout, and retry maxima", () => {
    // Given: a settings update at every server boundary.
    const payload = {
      version: settings.version,
      scheduler: {
        paused: false,
        default_compute_slots: 65_535,
        prefetch_per_worker: 65_535,
        max_concurrent_uploads: 65_535,
        max_concurrent_downloads: 65_535,
      },
      timeouts: { health_seconds: 604_800, poll_seconds: 604_800, transfer_seconds: 604_800 },
      retry: { initial_seconds: 604_800, maximum_seconds: 604_800, max_attempts: 100 },
    }

    // When: the update is parsed.
    const parsed = settingsUpdateRequestSchema.safeParse(payload)

    // Then: every exact maximum remains submit-capable.
    expect(parsed.success).toBe(true)
  })

  type SettingsPatch = Partial<Pick<SettingsUpdateRequest, "scheduler" | "timeouts" | "retry">>
  const invalidCases: readonly (readonly [string, SettingsPatch])[] = [
    ["zero upload limit", { scheduler: { ...settings.scheduler, max_concurrent_uploads: 0 } }],
    ["excess prefetch", { scheduler: { ...settings.scheduler, prefetch_per_worker: 65_536 } }],
    ["zero timeout", { timeouts: { ...settings.timeouts, health_seconds: 0 } }],
    ["excess timeout", { timeouts: { ...settings.timeouts, transfer_seconds: 604_801 } }],
    ["zero attempts", { retry: { ...settings.retry, max_attempts: 0 } }],
    ["excess attempts", { retry: { ...settings.retry, max_attempts: 101 } }],
    ["inverted delay", { retry: { ...settings.retry, initial_seconds: 31 } }],
  ]

  it.each(invalidCases)("rejects %s", (_case, patch) => {
    // Given: one update value outside the Rust validation contract.
    const payload = {
      version: settings.version,
      scheduler: patch.scheduler ?? settings.scheduler,
      timeouts: patch.timeouts ?? settings.timeouts,
      retry: patch.retry ?? settings.retry,
    }

    // When: the request is checked before submission.
    const parsed = settingsUpdateRequestSchema.safeParse(payload)

    // Then: the invalid mutation stays client-side.
    expect(parsed.success).toBe(false)
  })

  it("parses readiness checks without inventing fixed check names", () => {
    // Given: public readiness data with optional failure detail.
    const payload = {
      status: "not_ready",
      checks: [{ name: "persistence", ready: false, message: "database unavailable" }],
    }

    // When/Then: the open check names and closed readiness state are accepted.
    expect(readinessSchema.safeParse(payload).success).toBe(true)
  })
})
