import { z } from "zod"

const unsignedIntegerSchema = z.number().int().nonnegative().max(Number.MAX_SAFE_INTEGER)
const positiveU16Schema = z.number().int().min(1).max(65_535)
const durationSchema = z.number().int().min(1).max(604_800)

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
    scheduler: schedulerStatusSchema,
    timeouts: timeoutSettingsSchema,
    retry: retrySettingsSchema,
  })
  .strict()

export const settingsResponseSchema = settingsUpdateRequestSchema
  .extend({
    paths: z
      .object({
        input_roots: z.array(z.string()),
        output_roots: z.array(z.string()),
        data_root: z.string(),
        temp_root: z.string(),
        password_hash_file: z.string(),
      })
      .strict(),
    secure_cookie: z.boolean(),
    session_absolute_seconds: unsignedIntegerSchema,
    session_idle_seconds: unsignedIntegerSchema,
  })
  .strict()

export const readinessSchema = z
  .object({
    status: z.enum(["ready", "not_ready"]),
    checks: z.array(z.object({ name: z.string(), ready: z.boolean(), message: z.string().nullable() }).strict()),
  })
  .strict()

export type Readiness = z.infer<typeof readinessSchema>
export type SchedulerStatus = z.infer<typeof schedulerStatusSchema>
export type SettingsResponse = z.infer<typeof settingsResponseSchema>
export type SettingsUpdateRequest = z.infer<typeof settingsUpdateRequestSchema>
