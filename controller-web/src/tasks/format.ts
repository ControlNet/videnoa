import type { TaskStatus } from "../api/taskSchemas"

export function formatStatus(status: TaskStatus): string {
  return status.split("_").map(capitalize).join(" ")
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  const units = ["KB", "MB", "GB", "TB"] as const
  let value = bytes / 1024
  let index = 0
  while (value >= 1024 && index < units.length - 1) {
    value /= 1024
    index += 1
  }
  return `${value >= 10 ? value.toFixed(0) : value.toFixed(1)} ${units[index]}`
}

export function formatDuration(seconds: number | null): string {
  if (seconds === null) return "--"
  if (seconds < 60) return `${Math.round(seconds)}s`
  const minutes = Math.floor(seconds / 60)
  const remaining = Math.round(seconds % 60)
  return `${minutes}m ${remaining}s`
}

export function formatDate(value: string | null): string {
  if (value === null) return "--"
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(value))
}

function capitalize(value: string): string {
  return `${value.slice(0, 1).toLocaleUpperCase()}${value.slice(1)}`
}

/**
 * Compact relative time for dense rows.
 *
 * Rows carry the absolute timestamp in `title`; the cell itself stays narrow
 * enough that the task table fits a 1440px viewport without inline overflow.
 */
export function formatRelative(value: string | null, now: number = Date.now()): string {
  if (value === null) return "--"
  const timestamp = new Date(value).getTime()
  if (Number.isNaN(timestamp)) return "--"
  const seconds = Math.round((now - timestamp) / 1000)
  if (seconds < 0) return formatDate(value)
  if (seconds < 45) return "just now"
  const minutes = Math.round(seconds / 60)
  if (minutes < 60) return `${minutes}m ago`
  const hours = Math.round(minutes / 60)
  if (hours < 24) return `${hours}h ago`
  const days = Math.round(hours / 24)
  if (days < 30) return `${days}d ago`
  return formatDate(value)
}
