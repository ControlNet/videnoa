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
