import type { Task, TaskStatus, TaskStatusCounts } from "../api/taskSchemas"
import type { TaskQuery } from "./query"

export const activeStatuses = [
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
] as const satisfies readonly TaskStatus[]

const processingStatuses = [
  "uploading",
  "staged",
  "submitting",
  "processing",
  "remote_completed",
  "downloading",
  "verifying",
  "publishing",
  "remote_cleanup",
] as const satisfies readonly TaskStatus[]

export type CounterValues = {
  readonly all: number
  readonly active: number
  readonly queued: number
  readonly processing: number
  readonly failed: number
  readonly finished: number
}

export function counterValues(counts: TaskStatusCounts): CounterValues {
  const byStatus = new Map(counts.items.map(({ status, count }) => [status, count]))
  const sum = (statuses: readonly TaskStatus[]) =>
    statuses.reduce((total, status) => total + (byStatus.get(status) ?? 0), 0)

  return {
    all: counts.total,
    active: sum(activeStatuses),
    queued: byStatus.get("queued") ?? 0,
    processing: sum(processingStatuses),
    failed: byStatus.get("failed") ?? 0,
    finished: sum(["completed", "cancelled"]),
  }
}

export function isActiveStatus(status: TaskStatus): boolean {
  return activeStatuses.some((active) => active === status)
}

export function matchesTaskQuery(task: Task, query: TaskQuery): boolean {
  if (query.status !== "all" && task.status !== query.status) return false
  if (query.workflow !== "" && task.workflow !== query.workflow) return false
  if (query.worker !== "" && task.worker_id !== query.worker) return false
  const search = query.search.trim().toLocaleLowerCase()
  return search === "" || task.input_path.toLocaleLowerCase().includes(search) || task.output_path.toLocaleLowerCase().includes(search)
}

export function canMergeTaskUpdate(current: Task, incoming: Task, query: TaskQuery): boolean {
  if (incoming.version <= current.version) return false
  if (!isActiveStatus(current.status) || !isActiveStatus(incoming.status)) return false
  if (!matchesTaskQuery(incoming, query)) return false
  if (current.status !== incoming.status || current.worker_id !== incoming.worker_id) return false
  switch (query.sort) {
    case "priority":
      return current.priority === incoming.priority
    case "created_at":
      return current.created_at === incoming.created_at
    case "status":
    case "worker":
      return true
    case "completed_at":
    case "duration":
      return false
  }
}

export function taskName(task: Task): string {
  const segments = task.input_path.split(/[\\/]/)
  return segments.at(-1) || task.input_path
}
