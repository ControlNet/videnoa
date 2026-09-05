import { describe, expect, it } from "vitest"

import {
  readinessSchema,
  type SettingsUpdateRequest,
  settingsResponseSchema,
  settingsUpdateRequestSchema,
} from "./settingsSchemas"

const testOnlySettings = {
  version: 3,
  paths: {
    workspace: "/srv/videnoa/workspace",
    data_root: "/var/lib/videnoa",
    config_file: "/var/lib/videnoa/controller.toml",
  },
  server: { host: "0.0.0.0", port: 3001 },
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
  it("parses editable server and auth settings with safe read-only paths", () => {
    // Given: the exact GET /api/settings response.
    // When: it crosses the frontend boundary.
    const parsed = settingsResponseSchema.safeParse(testOnlySettings)

    // Then: editable configuration and safe path context remain structurally distinct.
    expect(parsed.success).toBe(true)
  })

  it("rejects removed root and password-hash response fields", () => {
    // Given: the prior response shape with fixed media roots and a credential file path.
    const legacy = {
      ...testOnlySettings,
      paths: {
        ...testOnlySettings.paths,
        input_roots: ["/synthetic/input"],
        output_roots: ["/synthetic/output"],
        password_hash_file: "/synthetic/password.phc",
      },
    }

    // When: the obsolete shape crosses the strict response boundary.
    const parsed = settingsResponseSchema.safeParse(legacy)

    // Then: browser-visible root and hash fields are refused.
    expect(parsed.success).toBe(false)
  })

  it("accepts exact scheduler, timeout, and retry maxima", () => {
    // Given: a settings update at every server boundary.
    const payload = {
      version: testOnlySettings.version,
      scheduler: {
        paused: false,
        default_compute_slots: 65_535,
        prefetch_per_worker: 65_535,
        max_concurrent_uploads: 65_535,
        max_concurrent_downloads: 65_535,
      },
      timeouts: { health_seconds: 604_800, poll_seconds: 604_800, transfer_seconds: 604_800 },
      retry: { initial_seconds: 604_800, maximum_seconds: 604_800, max_attempts: 100 },
      server: { host: "::", port: 65_535 },
      auth: { secure_cookie: false, session_absolute_seconds: 604_800, session_idle_seconds: 604_800 },
    }

    // When: the update is parsed.
    const parsed = settingsUpdateRequestSchema.safeParse(payload)

    // Then: every exact maximum remains submit-capable.
    expect(parsed.success).toBe(true)
  })

  type SettingsPatch = Partial<Pick<SettingsUpdateRequest, "scheduler" | "timeouts" | "retry" | "server" | "auth">>
  const invalidCases: readonly (readonly [string, SettingsPatch])[] = [
    ["zero upload limit", { scheduler: { ...testOnlySettings.scheduler, max_concurrent_uploads: 0 } }],
    ["excess prefetch", { scheduler: { ...testOnlySettings.scheduler, prefetch_per_worker: 65_536 } }],
    ["zero timeout", { timeouts: { ...testOnlySettings.timeouts, health_seconds: 0 } }],
    ["excess timeout", { timeouts: { ...testOnlySettings.timeouts, transfer_seconds: 604_801 } }],
    ["zero attempts", { retry: { ...testOnlySettings.retry, max_attempts: 0 } }],
    ["excess attempts", { retry: { ...testOnlySettings.retry, max_attempts: 101 } }],
    ["inverted delay", { retry: { ...testOnlySettings.retry, initial_seconds: 31 } }],
    ["zero server port", { server: { ...testOnlySettings.server, port: 0 } }],
    ["excess server port", { server: { ...testOnlySettings.server, port: 65_536 } }],
    ["blank server host", { server: { ...testOnlySettings.server, host: "" } }],
    ["zero absolute session", { auth: { secure_cookie: true, session_absolute_seconds: 0, session_idle_seconds: 1 } }],
    ["excess idle session", { auth: { secure_cookie: true, session_absolute_seconds: 604_800, session_idle_seconds: 604_801 } }],
    ["idle session above absolute", { auth: { secure_cookie: true, session_absolute_seconds: 60, session_idle_seconds: 61 } }],
  ]

  it.each(invalidCases)("rejects %s", (_case, patch) => {
    // Given: one update value outside the Rust validation contract.
    const payload = {
      version: testOnlySettings.version,
      scheduler: patch.scheduler ?? testOnlySettings.scheduler,
      timeouts: patch.timeouts ?? testOnlySettings.timeouts,
      retry: patch.retry ?? testOnlySettings.retry,
      server: patch.server ?? testOnlySettings.server,
      auth: patch.auth ?? {
        secure_cookie: testOnlySettings.secure_cookie,
        session_absolute_seconds: testOnlySettings.session_absolute_seconds,
        session_idle_seconds: testOnlySettings.session_idle_seconds,
      },
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
