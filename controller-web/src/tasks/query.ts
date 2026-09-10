import {
  type FailureStage,
  failureStageSchema,
  type TaskSource,
  type TaskStatus,
  taskSourceSchema,
  taskStatusSchema,
} from "../api/taskSchemas"

export const taskSorts = ["priority", "created_at", "completed_at", "status", "worker", "duration"] as const
export type TaskSort = (typeof taskSorts)[number]

export const taskOrders = ["asc", "desc"] as const
export type TaskOrder = (typeof taskOrders)[number]

export const taskLimits = [25, 50, 100] as const
export type TaskLimit = (typeof taskLimits)[number]

export const optionalColumns = [
  "priority",
  "source",
  "input_path",
  "output_path",
  "attempts",
  "duration",
  "failure_stage",
  "failure",
  "error",
  "remote_job_id",
] as const
export type OptionalColumn = (typeof optionalColumns)[number]

export const optionalColumnLabels = {
  priority: "Priority",
  source: "Source",
  input_path: "Input Path",
  output_path: "Output Path",
  attempts: "Attempts",
  duration: "Duration",
  failure_stage: "Failure Stage",
  failure: "Failure",
  error: "Error",
  remote_job_id: "Remote Job ID",
} as const satisfies Record<OptionalColumn, string>

export type TaskQuery = {
  readonly status: TaskStatus | "all"
  readonly source: TaskSource | "all"
  readonly failureStage: FailureStage | "all"
  readonly workflow: string
  readonly worker: string
  readonly search: string
  readonly sort: TaskSort
  readonly order: TaskOrder
  readonly limit: TaskLimit
  readonly offset: number
  readonly columns: readonly OptionalColumn[]
}

const defaults: TaskQuery = {
  status: "all",
  source: "all",
  failureStage: "all",
  workflow: "",
  worker: "",
  search: "",
  sort: "priority",
  order: "desc",
  limit: 50,
  offset: 0,
  columns: [],
}

export const taskViewStorageKey = "videnoa.tasks.view.v1"

type StoredTaskView = Omit<TaskQuery, "search" | "offset">

export function loadTaskQuery(storage: Pick<Storage, "getItem">): TaskQuery {
  let value: unknown
  try {
    const stored = storage.getItem(taskViewStorageKey)
    if (stored === null) return defaults
    value = JSON.parse(stored)
  } catch {
    return defaults
  }
  if (!isRecord(value)) return defaults

  const parsedStatus = taskStatusSchema.safeParse(value["status"])
  const parsedSource = taskSourceSchema.safeParse(value["source"])
  const parsedFailureStage = failureStageSchema.safeParse(value["failureStage"])
  return {
    status: value["status"] === "all" ? "all" : parsedStatus.success ? parsedStatus.data : defaults.status,
    source: value["source"] === "all" ? "all" : parsedSource.success ? parsedSource.data : defaults.source,
    failureStage: value["failureStage"] === "all" ? "all" : parsedFailureStage.success ? parsedFailureStage.data : defaults.failureStage,
    workflow: typeof value["workflow"] === "string" ? value["workflow"] : defaults.workflow,
    worker: typeof value["worker"] === "string" ? value["worker"] : defaults.worker,
    search: defaults.search,
    sort: parseMember(value["sort"], taskSorts, defaults.sort),
    order: parseMember(value["order"], taskOrders, defaults.order),
    limit: taskLimits.find((candidate) => candidate === value["limit"]) ?? defaults.limit,
    offset: defaults.offset,
    columns: parseColumns(value["columns"]),
  }
}

export function persistTaskQuery(storage: Pick<Storage, "setItem">, query: TaskQuery): void {
  const view: StoredTaskView = {
    status: query.status,
    source: query.source,
    failureStage: query.failureStage,
    workflow: query.workflow,
    worker: query.worker,
    sort: query.sort,
    order: query.order,
    limit: query.limit,
    columns: query.columns,
  }
  try {
    storage.setItem(taskViewStorageKey, JSON.stringify(view))
  } catch {
    // The current in-memory view remains usable when browser storage is unavailable.
  }
}

/** Reads an old Tasks link once before the page removes its query string. */
export function parseTaskQuery(parameters: URLSearchParams): TaskQuery {
  const parsedStatus = taskStatusSchema.safeParse(parameters.get("status"))
  const parsedSource = taskSourceSchema.safeParse(parameters.get("source"))
  const parsedFailureStage = failureStageSchema.safeParse(parameters.get("failure_stage"))
  const parsedLimit = Number(parameters.get("limit"))
  const parsedOffset = Number(parameters.get("offset"))
  const limit = taskLimits.find((candidate) => candidate === parsedLimit) ?? defaults.limit
  const columns = (parameters.get("columns") ?? "")
    .split(",")
    .filter((value): value is OptionalColumn => optionalColumns.some((column) => column === value))

  return {
    status: parsedStatus.success ? parsedStatus.data : defaults.status,
    source: parsedSource.success ? parsedSource.data : defaults.source,
    failureStage: parsedFailureStage.success ? parsedFailureStage.data : defaults.failureStage,
    workflow: parameters.get("workflow") ?? defaults.workflow,
    worker: parameters.get("worker") ?? defaults.worker,
    search: parameters.get("search") ?? defaults.search,
    sort: parseMember(parameters.get("sort"), taskSorts, defaults.sort),
    order: parseMember(parameters.get("order"), taskOrders, defaults.order),
    limit,
    offset: Number.isSafeInteger(parsedOffset) && parsedOffset >= 0 ? parsedOffset : defaults.offset,
    columns,
  }
}

export function taskPagePath(query: TaskQuery): string {
  const parameters = new URLSearchParams({
    limit: String(query.limit),
    offset: String(query.offset),
    sort: query.sort,
    direction: query.order,
  })
  if (query.status !== "all") parameters.set("status", query.status)
  if (query.source !== "all") parameters.set("source", query.source)
  if (query.failureStage !== "all") parameters.set("failure_stage", query.failureStage)
  if (query.workflow !== "") parameters.set("workflow", query.workflow)
  if (query.worker !== "") parameters.set("worker_id", query.worker)
  if (query.search !== "") parameters.set("search", query.search)
  return `api/tasks?${parameters.toString()}`
}

export function canonicalLastOffset(total: number, limit: TaskLimit): number {
  return total === 0 ? 0 : Math.floor((total - 1) / limit) * limit
}

function parseMember<const T extends string>(value: unknown, members: readonly T[], fallback: T): T {
  return members.find((member) => member === value) ?? fallback
}

function parseColumns(value: unknown): readonly OptionalColumn[] {
  if (!Array.isArray(value)) return defaults.columns
  return value.filter((candidate, index): candidate is OptionalColumn => (
    typeof candidate === "string"
    && optionalColumns.some((column) => column === candidate)
    && value.indexOf(candidate) === index
  ))
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value)
}
