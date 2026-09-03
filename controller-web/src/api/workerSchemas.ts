import { z } from "zod"

import { taskProgressSchema } from "./taskSchemas"

const unsignedIntegerSchema = z.number().int().nonnegative().max(Number.MAX_SAFE_INTEGER)
const positiveU16Schema = z.number().int().min(1).max(65_535)

export const workerApiUrlSchema = z.string().refine((value) => {
  if (!URL.canParse(value)) return false
  const url = new URL(value)
  return (url.protocol === "http:" || url.protocol === "https:")
    && url.username === ""
    && url.password === ""
    && url.search === ""
    && url.hash === ""
}, "Enter a credential-free HTTP(S) base URL without a query or fragment.")

export const workerCreateRequestSchema = z
  .object({
    name: z.string().refine((value) => value.trim().length > 0, "Enter a worker name."),
    api_url: workerApiUrlSchema,
    enabled: z.boolean(),
    compute_slots: positiveU16Schema,
  })
  .strict()

export const workerUpdateRequestSchema = workerCreateRequestSchema
  .extend({ version: unsignedIntegerSchema })
  .strict()

const workflowSummarySchema = z
  .object({
    name: z.string(),
    kind: z.enum(["workflow", "preset"]),
  })
  .strict()

const workerCapacitySchema = z
  .object({
    used_slots: z.number().int().nonnegative().max(65_535),
    available_slots: z.number().int().nonnegative().max(65_535),
    assigned_tasks: z.number().int().nonnegative().max(4_294_967_295),
    staged_tasks: z.number().int().nonnegative().max(4_294_967_295),
    processing_tasks: z.number().int().nonnegative().max(4_294_967_295),
    active_uploads: z.number().int().nonnegative().max(65_535),
    active_downloads: z.number().int().nonnegative().max(65_535),
    progress: taskProgressSchema.nullable(),
  })
  .strict()

export const workerSchema = z
  .object({
    id: z.string().uuid(),
    version: unsignedIntegerSchema,
    name: z.string(),
    api_url: workerApiUrlSchema,
    enabled: z.boolean(),
    online: z.boolean(),
    compute_slots: positiveU16Schema,
    capabilities: z
      .object({
        workflows: z.array(workflowSummarySchema),
        refreshed_at: z.iso.datetime().nullable(),
      })
      .strict(),
    capacity: workerCapacitySchema,
    last_seen_at: z.iso.datetime().nullable(),
    last_assigned_at: z.iso.datetime().nullable(),
    created_at: z.iso.datetime(),
    updated_at: z.iso.datetime(),
    last_error: z.string().nullable(),
  })
  .strict()

export const workerListSchema = z
  .object({
    items: z.array(workerSchema),
    total: unsignedIntegerSchema,
  })
  .strict()

export const workerDeleteResponseSchema = z
  .object({
    worker_id: z.string().uuid(),
    deleted: z.boolean(),
  })
  .strict()

export type Worker = z.infer<typeof workerSchema>
export type WorkerCreateRequest = z.infer<typeof workerCreateRequestSchema>
export type WorkerList = z.infer<typeof workerListSchema>
export type WorkerUpdateRequest = z.infer<typeof workerUpdateRequestSchema>
