import { z } from "zod"

const unsignedIntegerSchema = z.number().int().nonnegative().max(Number.MAX_SAFE_INTEGER)
const positiveU16Schema = z.number().int().min(1).max(65_535)
const durationSchema = z.number().int().min(1).max(604_800)

export const serverSettingsSchema = z
  .object({
    host: z.union([z.ipv4(), z.ipv6()]),
    port: positiveU16Schema,
  })
  .strict()

export const authSettingsSchema = z
  .object({
    secure_cookie: z.boolean(),
    session_absolute_seconds: durationSchema,
    session_idle_seconds: durationSchema,
  })
  .strict()
  .refine((auth) => auth.session_idle_seconds <= auth.session_absolute_seconds, {
    message: "Idle lifetime must not exceed absolute lifetime.",
    path: ["session_idle_seconds"],
  })

export const schedulerStatusSchema = z
  .object({
    paused: z.boolean(),
    default_compute_slots: positiveU16Schema,
    prefetch_per_worker: z.number().int().min(0).max(65_535),
    max_concurrent_uploads: positiveU16Schema,
    max_concurrent_downloads: positiveU16Schema,
  })
  .strict()

export const timeoutSettingsSchema = z
  .object({
    health_seconds: durationSchema,
    poll_seconds: durationSchema,
    transfer_seconds: durationSchema,
  })
  .strict()

export const retrySettingsSchema = z
  .object({
    initial_seconds: durationSchema,
    maximum_seconds: durationSchema,
    max_attempts: z.number().int().min(1).max(100),
  })
  .strict()
  .refine((retry) => retry.initial_seconds <= retry.maximum_seconds, {
    message: "Initial delay must not exceed maximum delay.",
    path: ["initial_seconds"],
  })

export const settingsUpdateRequestSchema = z
  .object({
    version: unsignedIntegerSchema,
    server: serverSettingsSchema,
    auth: authSettingsSchema,
    scheduler: schedulerStatusSchema,
    timeouts: timeoutSettingsSchema,
    retry: retrySettingsSchema,
  })
  .strict()

export const settingsResponseSchema = z
  .object({
    version: unsignedIntegerSchema,
    paths: z
      .object({
        workspace: z.string(),
        data_root: z.string(),
        config_file: z.string(),
      })
      .strict(),
    server: serverSettingsSchema,
    secure_cookie: z.boolean(),
    session_absolute_seconds: durationSchema,
    session_idle_seconds: durationSchema,
    scheduler: schedulerStatusSchema,
    timeouts: timeoutSettingsSchema,
    retry: retrySettingsSchema,
  })
  .strict()

export const readinessSchema = z
  .object({
    status: z.enum(["ready", "not_ready"]),
    checks: z.array(z.object({ name: z.string(), ready: z.boolean(), message: z.string().nullable() }).strict()),
  })
  .strict()

export type Readiness = z.infer<typeof readinessSchema>
export type ServerSettings = z.infer<typeof serverSettingsSchema>
export type SchedulerStatus = z.infer<typeof schedulerStatusSchema>
export type SettingsResponse = z.infer<typeof settingsResponseSchema>
export type SettingsUpdateRequest = z.infer<typeof settingsUpdateRequestSchema>
