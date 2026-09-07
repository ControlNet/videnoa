import { useEffect, useRef, useState, useSyncExternalStore } from "react"

import type { ApiClient } from "../api/client"
import { workerListSchema } from "../api/workerSchemas"
import { appInvalidationStore } from "../events/store"
import { appWorkerUpdateStore } from "../events/workerUpdates"

const noWorkerNames: ReadonlyMap<string, string> = new Map()

/**
 * Resolves worker identifiers to their registered names.
 *
 * Tasks carry only `worker_id`, but an operator reads names. `GET /api/workers`
 * stays the authority for worker identity, so this bounded read is the mapping
 * source and refreshes with the same invalidation the Workers route uses.
 *
 * A failure here is deliberately silent: the worker list is decoration for the
 * task table, and callers fall back to the identifier. Losing it must never
 * degrade the task history itself.
 */
export function useWorkerNames(apiClient: ApiClient): ReadonlyMap<string, string> {
  const invalidation = useSyncExternalStore(appInvalidationStore.subscribe, appInvalidationStore.snapshot)
  const update = useSyncExternalStore(appWorkerUpdateStore.subscribe, appWorkerUpdateStore.snapshot)
  const appliedUpdateGeneration = useRef(appWorkerUpdateStore.snapshot().generation)
  const [names, setNames] = useState<ReadonlyMap<string, string>>(noWorkerNames)

  useEffect(() => {
    void invalidation.generation
    const controller = new AbortController()
    void apiClient.request("api/workers", { schema: workerListSchema, signal: controller.signal }).then(
      (workers) => {
        if (controller.signal.aborted) return
        setNames(new Map(workers.items.map((worker) => [worker.id, worker.name])))
      },
      () => {
        if (controller.signal.aborted) return
        setNames(noWorkerNames)
      },
    )
    return () => controller.abort()
  }, [apiClient, invalidation.generation])

  /*
   * A worker delta names exactly one worker, which is the whole content of this
   * map, so it is applied without a request. An identical name returns the same
   * map instance: renaming nothing must not re-render the task table.
   */
  useEffect(() => {
    if (update.generation <= appliedUpdateGeneration.current) return
    appliedUpdateGeneration.current = update.generation
    const incoming = update.worker
    if (incoming === null) return
    queueMicrotask(() => {
      setNames((current) => {
        if (current.get(incoming.id) === incoming.name) return current
        const next = new Map(current)
        next.set(incoming.id, incoming.name)
        return next
      })
    })
  }, [update.generation, update.worker])

  return names
}
