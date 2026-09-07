import { z } from "zod"

export const taskStatusSchema = z.enum([
  "queued",
  "reserved",
  "uploading",
  "staged",
  "submitting",
  "processing",
  "remote_completed",
  "downloading",
  "verifying",
  "publishing",
  "remote_cleanup",
  "completed",
  "failed",
  "cancelled",
])

export const taskSourceSchema = z.enum(["manual", "api"])

export const taskProgressSchema = z
  .object({
    percent: z.number().min(0).max(100),
    processed_frames: z.number().int().nonnegative().nullable(),
    total_frames: z.number().int().nonnegative().nullable(),
    frames_per_second: z.number().nonnegative().nullable(),
    eta_seconds: z.number().nonnegative().nullable(),
    bytes_transferred: z.number().int().nonnegative().nullable(),
    bytes_total: z.number().int().nonnegative().nullable(),
  })
  .strict()

export const failureStageSchema = z.enum([
  "reservation",
  "upload",
  "submission",
  "processing",
  "download",
  "verification",
  "publication",
  "local_cleanup",
  "remote_cleanup",
])

export const failureCodeSchema = z.enum([
  "input_unavailable",
  "input_changed",
  "output_exists",
  "worker_unavailable",
  "workflow_incompatible",
  "transfer_failed",
  "remote_submission_failed",
  "remote_state_ambiguous",
  "processing_failed",
  "verification_failed",
  "publication_failed",
  "publication_ambiguous",
  "cleanup_failed",
  "cancelled",
])

export const taskFailureSchema = z
  .object({
    failure_stage: failureStageSchema,
    failure_code: failureCodeSchema,
    message: z.string(),
    retryable: z.boolean(),
  })
  .strict()

export const taskSchema = z
  .object({
    id: z.string().uuid(),
    version: z.number().int().nonnegative(),
    status: taskStatusSchema,
    input_path: z.string(),
    output_path: z.string(),
    input_extension: z.string(),
    output_extension: z.string(),
    workflow: z.string(),
    priority: z.number().int(),
    source: taskSourceSchema,
    source_reference: z.string().nullable(),
    input_size: z.number().int().nonnegative(),
    worker_id: z.string().uuid().nullable(),
    remote_job_id: z.string().uuid().nullable(),
    progress: taskProgressSchema,
    attempt_count: z.number().int().nonnegative(),
    failure: taskFailureSchema.nullable(),
    cancel_requested_at: z.iso.datetime().nullable(),
    created_at: z.iso.datetime(),
    updated_at: z.iso.datetime(),
    completed_at: z.iso.datetime().nullable(),
  })
  .strict()

export const taskCreateRequestSchema = z
  .object({
    input_path: z.string().min(1),
    output_path: z.string().min(1),
    workflow: z.string().min(1),
    priority: z.number().int(),
    source: z.literal("manual"),
    source_reference: z.null(),
  })
  .strict()

export const taskAttemptSchema = z
  .object({
    id: z.string().uuid(),
    task_id: z.string().uuid(),
    attempt_number: z.number().int().nonnegative(),
    worker_id: z.string().uuid().nullable(),
    status: taskStatusSchema,
    submission_key: z.string().uuid(),
    remote_job_id: z.string().uuid().nullable(),
    remote_input_path: z.string().nullable(),
    remote_output_path: z.string().nullable(),
    progress: taskProgressSchema,
    retry: z
      .object({
        retry_count: z.number().int().nonnegative(),
        next_retry_at: z.iso.datetime().nullable(),
      })
      .strict(),
    failure: taskFailureSchema.nullable(),
    created_at: z.iso.datetime(),
    started_at: z.iso.datetime().nullable(),
    completed_at: z.iso.datetime().nullable(),
  })
  .strict()

export const taskDetailSchema = z
  .object({
    task: taskSchema,
    attempts: z.array(taskAttemptSchema),
    total: z.number().int().nonnegative(),
    limit: z.number().int().positive().max(500),
    offset: z.number().int().nonnegative(),
  })
  .strict()

export const cancelTaskResponseSchema = z
  .object({
    task_id: z.string().uuid(),
    status: taskStatusSchema,
    cancel_requested_at: z.iso.datetime(),
  })
  .strict()

export const retryTaskResponseSchema = z
  .object({
    task_id: z.string().uuid(),
    attempt_id: z.string().uuid(),
    status: taskStatusSchema,
  })
  .strict()

export const taskCreateResponseSchema = taskSchema

export const taskListSchema = z
  .object({
    items: z.array(taskSchema),
    total: z.number().int().nonnegative(),
    limit: z.number().int().positive().max(100),
    offset: z.number().int().nonnegative(),
  })
  .strict()

export const taskStatusCountsSchema = z
  .object({
    items: z.array(z.object({ status: taskStatusSchema, count: z.number().int().nonnegative() }).strict()),
    total: z.number().int().nonnegative(),
  })
  .strict()

export const taskUpdatedEventSchema = z
  .object({
    type: z.literal("task_updated"),
    data: z.object({ event_id: z.string().uuid(), task: taskSchema }).strict(),
  })
  .strict()

export type Task = z.infer<typeof taskSchema>
export type FailureCode = z.infer<typeof failureCodeSchema>
export type FailureStage = z.infer<typeof failureStageSchema>
export type TaskSource = z.infer<typeof taskSourceSchema>
export type TaskAttempt = z.infer<typeof taskAttemptSchema>
export type TaskCreateRequest = z.infer<typeof taskCreateRequestSchema>
export type TaskDetail = z.infer<typeof taskDetailSchema>
export type TaskList = z.infer<typeof taskListSchema>
export type TaskStatus = z.infer<typeof taskStatusSchema>
export type TaskStatusCounts = z.infer<typeof taskStatusCountsSchema>
