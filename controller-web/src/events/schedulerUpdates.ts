import type { SchedulerStatus } from "../api/settingsSchemas"

export type SchedulerUpdateSnapshot = {
  readonly generation: number
  readonly scheduler: SchedulerStatus | null
}

export type SchedulerUpdateStore = {
  readonly publish: (scheduler: SchedulerStatus) => void
  readonly snapshot: () => SchedulerUpdateSnapshot
  readonly subscribe: (subscriber: () => void) => () => void
}

/**
 * Scheduler deltas from the event stream, kept off the invalidation store.
 *
 * Pausing the scheduler must not invalidate task queries or the worker list, so
 * it travels on its own channel exactly as task and worker deltas do.
 */
export function createSchedulerUpdateStore(): SchedulerUpdateStore {
  let snapshot: SchedulerUpdateSnapshot = { generation: 0, scheduler: null }
  const subscribers = new Set<() => void>()

  return {
    publish: (scheduler) => {
      snapshot = { generation: snapshot.generation + 1, scheduler }
      for (const subscriber of subscribers) subscriber()
    },
    snapshot: () => snapshot,
    subscribe: (subscriber) => {
      subscribers.add(subscriber)
      return () => subscribers.delete(subscriber)
    },
  }
}

export const appSchedulerUpdateStore = createSchedulerUpdateStore()
