import { z } from "zod"

import { taskStatusSchema } from "../api/taskSchemas"

export const abandonTaskResponseSchema = z
  .object({
    task_id: z.string().uuid(),
    status: taskStatusSchema,
  })
  .strict()

export const discardTaskResponseSchema = z
  .object({
    task_id: z.string().uuid(),
    status: taskStatusSchema,
    worker_folder_deleted: z.boolean(),
  })
  .strict()
