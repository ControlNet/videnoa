import { useEffect, useRef, useState, useSyncExternalStore } from "react"

import type { ApiClient } from "../api/client"
import { type Worker, workerListSchema } from "../api/workerSchemas"
import { appInvalidationStore } from "../events/store"
import { deltasSince } from "../events/deltas"
import { appWorkerUpdateStore } from "../events/workerUpdates"

const noWorkerNames: ReadonlyMap<string, string> = new Map()
const noWorkerVersions: ReadonlyMap<string, number> = new Map()

type WorkerNameState = {
  readonly names: ReadonlyMap<string, string>
  readonly versions: ReadonlyMap<string, number>
}

const initialWorkerNameState: WorkerNameState = {
  names: noWorkerNames,
  versions: noWorkerVersions,
}

function mapsEqual(left: ReadonlyMap<string, string>, right: ReadonlyMap<string, string>): boolean {
  if (left.size !== right.size) return false
  for (const [id, name] of left) {
    if (!right.has(id) || right.get(id) !== name) return false
  }
  return true
}

function applyWorkerUpdates(current: WorkerNameState, values: readonly Worker[]): WorkerNameState {
  const latestByWorker = new Map<string, { readonly name: string; readonly version: number }>()
  for (const incoming of values) {
    const knownVersion = latestByWorker.get(incoming.id)?.version ?? current.versions.get(incoming.id)
    if (knownVersion !== undefined && incoming.version < knownVersion) continue
    latestByWorker.set(incoming.id, { name: incoming.name, version: incoming.version })
  }
  if (latestByWorker.size === 0) return current

  let namesChanged = false
  let versionsChanged = false
  for (const [id, entry] of latestByWorker) {
    if (!current.names.has(id) || current.names.get(id) !== entry.name) {
      namesChanged = true
    }
    if (current.versions.get(id) !== entry.version) {
      versionsChanged = true
    }
  }
  if (!namesChanged && !versionsChanged) return current

  const nextVersions = new Map(current.versions)
  for (const [id, entry] of latestByWorker) nextVersions.set(id, entry.version)
  if (!namesChanged) {
    return { names: current.names, versions: nextVersions }
  }

  const nextNames = new Map(current.names)
  for (const [id, entry] of latestByWorker) nextNames.set(id, entry.name)
  return { names: nextNames, versions: nextVersions }
}

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
  const [state, setState] = useState<WorkerNameState>(initialWorkerNameState)
  const [reloadGeneration, setReloadGeneration] = useState(0)

  useEffect(() => {
    void invalidation.generation
    void reloadGeneration
    const controller = new AbortController()
    const requestStartGeneration = appWorkerUpdateStore.snapshot().generation
    void apiClient.request("api/workers", { schema: workerListSchema, signal: controller.signal }).then(
      (workers) => {
        if (controller.signal.aborted) return
        const latestUpdate = appWorkerUpdateStore.snapshot()
        const inFlightDeltas =
          latestUpdate.generation > requestStartGeneration
            ? deltasSince(latestUpdate.recent, requestStartGeneration)
            : { values: [] as readonly Worker[], complete: true }
        if (latestUpdate.generation > appliedUpdateGeneration.current) {
          appliedUpdateGeneration.current = latestUpdate.generation
        }
        if (!inFlightDeltas.complete) {
          setReloadGeneration((generation) => generation + 1)
          return
        }
        setState((current) => {
          const mergedEntries = new Map<string, { readonly name: string; readonly version: number }>()
          for (const item of workers.items) {
            const currentVersion = current.versions.get(item.id)
            const currentName = current.names.get(item.id)
            if (currentVersion !== undefined && currentName !== undefined && currentVersion > item.version) {
              mergedEntries.set(item.id, { name: currentName, version: currentVersion })
            } else {
              mergedEntries.set(item.id, { name: item.name, version: item.version })
            }
          }
          for (const incoming of inFlightDeltas.values) {
            const existing = mergedEntries.get(incoming.id)
            if (existing === undefined || incoming.version >= existing.version) {
              mergedEntries.set(incoming.id, { name: incoming.name, version: incoming.version })
            }
          }
          const nextNames = new Map<string, string>()
          const nextVersions = new Map<string, number>()
          for (const [id, entry] of mergedEntries) {
            nextNames.set(id, entry.name)
            nextVersions.set(id, entry.version)
          }
          return {
            names: mapsEqual(current.names, nextNames) ? current.names : nextNames,
            versions: nextVersions,
          }
        })
      },
      () => {
        if (controller.signal.aborted) return
        setState(initialWorkerNameState)
      },
    )
    return () => controller.abort()
  }, [apiClient, invalidation.generation, reloadGeneration])

  /*
   * A worker delta names exactly one worker, which is the whole content of this
   * map, so it is applied without a request. An identical name returns the same
   * map instance: renaming nothing must not re-render the task table.
   */
  useEffect(() => {
    const applied = appliedUpdateGeneration.current
    if (update.generation <= applied) return
    appliedUpdateGeneration.current = update.generation
    const { values, complete } = deltasSince(update.recent, applied)
    if (!complete) {
      queueMicrotask(() => setReloadGeneration((generation) => generation + 1))
      return
    }
    if (values.length === 0) return
    queueMicrotask(() => {
      setState((current) => applyWorkerUpdates(current, values))
    })
  }, [update.generation, update.recent])

  return state.names
}
