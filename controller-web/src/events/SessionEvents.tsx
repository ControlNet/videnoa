import { useEffect } from "react"

import { taskUpdatedEventSchema } from "../api/taskSchemas"
import { appInvalidationStore } from "./store"
import { appTaskUpdateStore } from "./taskUpdates"

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
