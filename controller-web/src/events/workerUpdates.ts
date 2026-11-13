import type { Worker } from "../api/workerSchemas"
import { appendDelta, type Delta } from "./deltas"

export type WorkerUpdateSnapshot = {
  readonly generation: number
  /** The latest worker; `recent` holds the ones before it too. */
  readonly worker: Worker | null
  readonly recent: readonly Delta<Worker>[]
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
  let snapshot: WorkerUpdateSnapshot = { generation: 0, worker: null, recent: [] }
  const subscribers = new Set<() => void>()

  return {
    publish: (worker) => {
      const generation = snapshot.generation + 1
      snapshot = { generation, worker, recent: appendDelta(snapshot.recent, { generation, value: worker }) }
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
