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

/*
 * Clock-skew tolerance.
 *
 * The Controller stamps these timestamps and the browser renders them, so only
 * one of the two clocks is the local one. A machine that has drifted or just
 * woken puts a freshly stamped value a moment in the future; within this window
 * that is skew, not information.
 */
const skewToleranceSeconds = 60

/**
 * Compact relative time for dense rows.
 *
 * Every result stays on the relative scale, and every form fits in eight
 * characters. That is the point: this cell sits in an auto-layout table, so a
 * wide result does not merely widen its own column -- it moves every column
 * beside it. A value falling out of the relative scale into a full timestamp
 * cost 106px of column width and shifted the table by 77px, which read as the
 * row flickering. Rows carry the absolute timestamp in `title` instead, so
 * precision stays one hover away rather than in the layout.
 */
export function formatRelative(value: string | null, now: number = Date.now()): string {
  if (value === null) return "--"
  const timestamp = new Date(value).getTime()
  if (Number.isNaN(timestamp)) return "--"
  const seconds = Math.round((now - timestamp) / 1000)
  // Past plausible skew the clocks genuinely disagree, which is worth showing --
  // but as a direction on the same scale, not as a wider kind of value.
  if (seconds < -skewToleranceSeconds) return `in ${elapsed(-seconds)}`
  if (seconds < 45) return "just now"
  return `${elapsed(seconds)} ago`
}

/** Coarsest unit that keeps the magnitude readable, from minutes to years. */
function elapsed(seconds: number): string {
  const minutes = Math.round(seconds / 60)
  if (minutes < 60) return `${minutes}m`
  const hours = Math.round(minutes / 60)
  if (hours < 24) return `${hours}h`
  const days = Math.round(hours / 24)
  if (days < 30) return `${days}d`
  const months = Math.round(days / 30)
  if (months < 12) return `${months}mo`
  return `${Math.round(months / 12)}y`
}
