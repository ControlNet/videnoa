import type { TaskStatus } from "../api/taskSchemas"
import type { Tone } from "../ui/Status"

/**
 * Fourteen durable statuses collapse to four operator-facing tones.
 *
 * The exact status stays in the row text; the tone answers the only question a
 * dense list is scanned for, which is whether a task is waiting, moving,
 * finished, or broken.
 */
export function taskTone(status: TaskStatus): Tone {
  switch (status) {
    case "queued":
    case "cancelled":
      return "quiet"
    case "completed":
      return "positive"
    case "failed":
      return "negative"
    default:
      return "active"
  }
}

export function workerHealthTone(online: boolean): Tone {
  return online ? "positive" : "negative"
}
