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

    let current: EventSource | null = null
    let disposeStream = () => {}
    let retryTimer: ReturnType<typeof setTimeout> | undefined
    let retryDelay = 1000
    let degraded = false
    let receivedSnapshotSignal = false

    function clearRetry() {
      clearTimeout(retryTimer)
      retryTimer = undefined
    }

    function canRecover() {
      return document.visibilityState === "visible" && navigator.onLine
    }

    function scheduleRetry() {
      if (retryTimer !== undefined || !canRecover()) return
      retryTimer = setTimeout(() => {
        retryTimer = undefined
        if (!canRecover()) return
        connect(true)
      }, retryDelay)
      retryDelay = Math.min(retryDelay * 2, 30000)
    }

    function connect(reconnecting: boolean) {
      clearRetry()
      disposeStream()
      if (reconnecting) onConnectionStateChange("reconnecting")
      const events = new EventSource("/api/events", { withCredentials: true })
      current = events
      degraded = false
      const listeners = new AbortController()
      const listen = (type: string, listener: (event: Event) => void) => {
        events.addEventListener(type, listener, { signal: listeners.signal })
      }
      disposeStream = () => {
        listeners.abort()
        events.close()
      }

      listen("open", () => {
        retryDelay = 1000
        degraded = false
        onConnectionStateChange("connected")
      })
      listen("refetch", () => {
        onConnectionStateChange("connected")
        appInvalidationStore.invalidate(receivedSnapshotSignal ? "lag" : "initial")
        receivedSnapshotSignal = true
      })
      listen("task_updated", (event) => {
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
      listen("worker_updated", (event) => {
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
      listen("scheduler_updated", (event) => {
        const scheduler = schedulerFromEvent(event)
        if (scheduler !== null) {
          onConnectionStateChange("connected")
          appSchedulerUpdateStore.publish(scheduler)
        } else {
          appInvalidationStore.invalidate("lag")
        }
      })
      listen("error", () => {
        degraded = true
        onConnectionStateChange(events.readyState === EventSource.CLOSED ? "unavailable" : "reconnecting")
        appInvalidationStore.invalidate("reconnect")
        if (events.readyState === EventSource.CLOSED) scheduleRetry()
      })
    }

    // Sleep can leave a terminal stream behind. Recover on return without
    // replacing healthy streams or duplicating a fresh connection attempt.
    function resume() {
      if (!canRecover() || current === null || current.readyState === EventSource.OPEN) return
      if (current.readyState !== EventSource.CLOSED && !degraded) return
      connect(true)
    }

    connect(false)
    document.addEventListener("visibilitychange", resume)
    window.addEventListener("focus", resume)
    window.addEventListener("online", resume)
    window.addEventListener("pageshow", resume)

    return () => {
      clearRetry()
      disposeStream()
      document.removeEventListener("visibilitychange", resume)
      window.removeEventListener("focus", resume)
      window.removeEventListener("online", resume)
      window.removeEventListener("pageshow", resume)
    }
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
