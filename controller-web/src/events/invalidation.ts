export const invalidationReasons = ["initial", "reconnect", "lag"] as const
export type InvalidationReason = (typeof invalidationReasons)[number]

export type InvalidationSnapshot = {
  readonly generation: number
  readonly reason: InvalidationReason | null
}

export type InvalidationStore = {
  readonly invalidate: (reason: InvalidationReason) => void
  readonly snapshot: () => InvalidationSnapshot
  readonly subscribe: (subscriber: () => void) => () => void
}

export function createInvalidationStore(): InvalidationStore {
  let snapshot: InvalidationSnapshot = { generation: 0, reason: null }
  const subscribers = new Set<() => void>()

  return {
    invalidate: (reason) => {
      snapshot = { generation: snapshot.generation + 1, reason }
      for (const subscriber of subscribers) subscriber()
    },
    snapshot: () => snapshot,
    subscribe: (subscriber) => {
      subscribers.add(subscriber)
      return () => subscribers.delete(subscriber)
    },
  }
}
