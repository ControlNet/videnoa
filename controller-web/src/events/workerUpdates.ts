import type { Worker } from "../api/workerSchemas"

export type WorkerUpdateSnapshot = {
  readonly generation: number
  readonly worker: Worker | null
}

export type WorkerUpdateStore = {
  readonly publish: (worker: Worker) => void
  readonly snapshot: () => WorkerUpdateSnapshot
  readonly subscribe: (subscriber: () => void) => () => void
}

/**
 * Worker deltas from the event stream, kept separate from the invalidation store.
 *
 * A worker's health, policy or capability change must not invalidate task
 * queries, so it travels on its own channel exactly as task deltas do.
 */
export function createWorkerUpdateStore(): WorkerUpdateStore {
  let snapshot: WorkerUpdateSnapshot = { generation: 0, worker: null }
  const subscribers = new Set<() => void>()

  return {
    publish: (worker) => {
      snapshot = { generation: snapshot.generation + 1, worker }
      for (const subscriber of subscribers) subscriber()
    },
    snapshot: () => snapshot,
    subscribe: (subscriber) => {
      subscribers.add(subscriber)
      return () => subscribers.delete(subscriber)
    },
  }
}

export const appWorkerUpdateStore = createWorkerUpdateStore()
