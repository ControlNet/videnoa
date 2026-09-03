import type { Task } from "../api/taskSchemas"

export type TaskUpdateSnapshot = {
  readonly generation: number
  readonly task: Task | null
}

export type TaskUpdateStore = {
  readonly publish: (task: Task) => void
  readonly snapshot: () => TaskUpdateSnapshot
  readonly subscribe: (subscriber: () => void) => () => void
}

export function createTaskUpdateStore(): TaskUpdateStore {
  let snapshot: TaskUpdateSnapshot = { generation: 0, task: null }
  const subscribers = new Set<() => void>()

  return {
    publish: (task) => {
      snapshot = { generation: snapshot.generation + 1, task }
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
