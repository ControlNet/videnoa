import { useEffect } from "react"

import { appInvalidationStore } from "./store"

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
    events.addEventListener("error", () => {
      onConnectionStateChange(events.readyState === EventSource.CLOSED ? "unavailable" : "reconnecting")
      appInvalidationStore.invalidate("reconnect")
    })

    return () => events.close()
  }, [onConnectionStateChange])

  return null
}
