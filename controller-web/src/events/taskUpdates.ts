import type { Task } from "../api/taskSchemas"
import { appendDelta, type Delta } from "./deltas"

export type TaskUpdateSnapshot = {
  readonly generation: number
  /** The latest task; `recent` holds the ones before it too. */
  readonly task: Task | null
  readonly recent: readonly Delta<Task>[]
}

export type TaskUpdateStore = {
  readonly publish: (task: Task) => void
  readonly snapshot: () => TaskUpdateSnapshot
  readonly subscribe: (subscriber: () => void) => () => void
}

export function createTaskUpdateStore(): TaskUpdateStore {
  let snapshot: TaskUpdateSnapshot = { generation: 0, task: null, recent: [] }
  const subscribers = new Set<() => void>()

  return {
    publish: (task) => {
      const generation = snapshot.generation + 1
      snapshot = { generation, task, recent: appendDelta(snapshot.recent, { generation, value: task }) }
      for (const subscriber of subscribers) subscriber()
    },
    snapshot: () => snapshot,
    subscribe: (subscriber) => {
      subscribers.add(subscriber)
      return () => subscribers.delete(subscriber)
    },
  }
}

export const appTaskUpdateStore = createTaskUpdateStore()
