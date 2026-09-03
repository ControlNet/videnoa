import { type TaskStatus, taskStatusSchema } from "../api/taskSchemas"

export const taskSorts = ["priority", "created_at", "completed_at", "status", "worker", "duration"] as const
export type TaskSort = (typeof taskSorts)[number]

export const taskOrders = ["asc", "desc"] as const
export type TaskOrder = (typeof taskOrders)[number]

export const taskLimits = [25, 50, 100] as const
export type TaskLimit = (typeof taskLimits)[number]

export const optionalColumns = ["path", "attempts", "duration", "failure", "error", "remote_job"] as const
export type OptionalColumn = (typeof optionalColumns)[number]

export type TaskQuery = {
  readonly status: TaskStatus | "all"
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
  workflow: "",
  worker: "",
  search: "",
  sort: "priority",
  order: "desc",
  limit: 50,
  offset: 0,
  columns: [],
}

export function parseTaskQuery(parameters: URLSearchParams): TaskQuery {
  const parsedStatus = taskStatusSchema.safeParse(parameters.get("status"))
  const parsedLimit = Number(parameters.get("limit"))
  const parsedOffset = Number(parameters.get("offset"))
  const limit = taskLimits.find((candidate) => candidate === parsedLimit) ?? defaults.limit
  const columns = (parameters.get("columns") ?? "")
    .split(",")
    .filter((value): value is OptionalColumn => optionalColumns.some((column) => column === value))

  return {
    status: parsedStatus.success ? parsedStatus.data : defaults.status,
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

export function serializeTaskQuery(query: TaskQuery): URLSearchParams {
  const parameters = new URLSearchParams()
  setMeaningful(parameters, "status", query.status, defaults.status)
  setMeaningful(parameters, "workflow", query.workflow, defaults.workflow)
  setMeaningful(parameters, "worker", query.worker, defaults.worker)
  setMeaningful(parameters, "search", query.search, defaults.search)
  setMeaningful(parameters, "sort", query.sort, defaults.sort)
  setMeaningful(parameters, "order", query.order, defaults.order)
  if (query.limit !== defaults.limit) parameters.set("limit", String(query.limit))
  if (query.offset !== defaults.offset) parameters.set("offset", String(query.offset))
  if (query.columns.length > 0) parameters.set("columns", query.columns.join(","))
  return parameters
}

export function taskPagePath(query: TaskQuery): string {
  const parameters = new URLSearchParams({
    limit: String(query.limit),
    offset: String(query.offset),
    sort: query.sort,
    direction: query.order,
  })
  if (query.status !== "all") parameters.set("status", query.status)
  if (query.workflow !== "") parameters.set("workflow", query.workflow)
  if (query.worker !== "") parameters.set("worker_id", query.worker)
  if (query.search !== "") parameters.set("search", query.search)
  return `api/tasks?${parameters.toString()}`
}

export function canonicalLastOffset(total: number, limit: TaskLimit): number {
  return total === 0 ? 0 : Math.floor((total - 1) / limit) * limit
}

function parseMember<const T extends string>(value: string | null, members: readonly T[], fallback: T): T {
  return members.find((member) => member === value) ?? fallback
}

function setMeaningful(parameters: URLSearchParams, key: string, value: string, fallback: string): void {
  if (value !== fallback) parameters.set(key, value)
}
