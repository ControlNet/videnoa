import { useEffect } from "react"

import { type SchedulerStatus, schedulerUpdatedEventSchema } from "../api/settingsSchemas"
import { taskUpdatedEventSchema } from "../api/taskSchemas"
import { type Worker, workerUpdatedEventSchema } from "../api/workerSchemas"
import { appSchedulerUpdateStore } from "./schedulerUpdates"
import { appInvalidationStore } from "./store"
import { appTaskUpdateStore } from "./taskUpdates"
import { appWorkerUpdateStore } from "./workerUpdates"

export type ConnectionState = "connecting" | "connected" | "reconnecting" | "unavailable"

type SessionEventsProps = {
  readonly onConnectionStateChange: (state: ConnectionState) => void
}

export function SessionEvents({ onConnectionStateChange }: SessionEventsProps) {
  useEffect(() => {
    if (typeof EventSource === "undefined") {
      onConnectionStateChange("unavailable")
      return
    }

    const events = new EventSource("/api/events", { withCredentials: true })
    let receivedSnapshotSignal = false

    events.addEventListener("open", () => {
      onConnectionStateChange("connected")
    })
    events.addEventListener("refetch", () => {
      onConnectionStateChange("connected")
      appInvalidationStore.invalidate(receivedSnapshotSignal ? "lag" : "initial")
      receivedSnapshotSignal = true
    })
    events.addEventListener("task_updated", (event) => {
      const task = taskFromEvent(event)
      if (task !== null) {
        onConnectionStateChange("connected")
        appTaskUpdateStore.publish(task)
      } else {
        appInvalidationStore.invalidate("lag")
      }
    })
    /*
     * The Controller publishes a full worker DTO whenever health, policy or
     * capabilities change. Without this listener EventSource dropped the event
     * silently, so the Workers route only ever showed the health it had at load
     * and an operator had to reload the page to see a worker come back online.
     */
    events.addEventListener("worker_updated", (event) => {
      const worker = workerFromEvent(event)
      if (worker !== null) {
        onConnectionStateChange("connected")
        appWorkerUpdateStore.publish(worker)
      } else {
        appInvalidationStore.invalidate("lag")
      }
    })
    /*
     * Pausing or reconfiguring the scheduler elsewhere -- another browser, or the
     * API -- reaches this session only through this listener. Without it the
     * Settings route kept showing the scheduler state it had at load.
     */
    events.addEventListener("scheduler_updated", (event) => {
      const scheduler = schedulerFromEvent(event)
      if (scheduler !== null) {
        onConnectionStateChange("connected")
        appSchedulerUpdateStore.publish(scheduler)
      } else {
        appInvalidationStore.invalidate("lag")
      }
    })
    events.addEventListener("error", () => {
      onConnectionStateChange(events.readyState === EventSource.CLOSED ? "unavailable" : "reconnecting")
      appInvalidationStore.invalidate("reconnect")
    })

    return () => events.close()
  }, [onConnectionStateChange])

  return null
}

function taskFromEvent(event: Event) {
  if (!(event instanceof MessageEvent) || typeof event.data !== "string") return null
  try {
    const parsed = taskUpdatedEventSchema.safeParse(JSON.parse(event.data))
    return parsed.success ? parsed.data.data.task : null
  } catch (error) {
    if (error instanceof SyntaxError) return null
    throw error
  }
}

function schedulerFromEvent(event: Event): SchedulerStatus | null {
  if (!(event instanceof MessageEvent) || typeof event.data !== "string") return null
  try {
    const parsed = schedulerUpdatedEventSchema.safeParse(JSON.parse(event.data))
    return parsed.success ? parsed.data.data.scheduler : null
  } catch (error) {
    if (error instanceof SyntaxError) return null
    throw error
  }
}

function workerFromEvent(event: Event): Worker | null {
  if (!(event instanceof MessageEvent) || typeof event.data !== "string") return null
  try {
    const parsed = workerUpdatedEventSchema.safeParse(JSON.parse(event.data))
    return parsed.success ? parsed.data.data.worker : null
  } catch (error) {
    if (error instanceof SyntaxError) return null
    throw error
  }
}
