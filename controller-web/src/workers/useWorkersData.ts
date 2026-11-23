import { useEffect, useRef, useState, useSyncExternalStore } from "react"

import { type ApiClient, ApiClientError } from "../api/client"
import {
  type Worker,
  type WorkerCreateRequest,
  type WorkerList,
  type WorkerUpdateRequest,
  workerDeleteResponseSchema,
  workerListSchema,
  workerSchema,
} from "../api/workerSchemas"
import { appInvalidationStore } from "../events/store"
import { appTaskUpdateStore } from "../events/taskUpdates"
import { deltasSince } from "../events/deltas"
import { appWorkerUpdateStore } from "../events/workerUpdates"

export type WorkersData = {
  readonly workers: WorkerList | null
  readonly loading: boolean
  readonly error: string | null
  readonly actionError: ApiClientError | null
  readonly mutating: boolean
  readonly retry: () => void
  readonly clearActionError: () => void
  readonly createWorker: (request: WorkerCreateRequest) => Promise<boolean>
  readonly updateWorker: (worker: Worker, request: WorkerUpdateRequest) => Promise<boolean>
  readonly setEnabled: (worker: Worker, enabled: boolean) => Promise<boolean>
  readonly deleteWorker: (worker: Worker) => Promise<boolean>
}

type WorkerDeltaReconciliation =
  | { readonly kind: "unchanged" }
  | { readonly kind: "merged"; readonly list: WorkerList }
  | { readonly kind: "refetch" }

function reconcileWorkerDeltas(
  list: WorkerList,
  values: readonly Worker[],
): WorkerDeltaReconciliation {
  if (values.some((incoming) => !list.items.some((worker) => worker.id === incoming.id))) {
    return { kind: "refetch" }
  }
  let items = list.items
  for (const incoming of values) {
    const latest = items.find((worker) => worker.id === incoming.id)
    if (latest === undefined || incoming.version <= latest.version) continue
    items = items.map((worker) => (worker.id === incoming.id ? incoming : worker))
  }
  if (items === list.items) return { kind: "unchanged" }
  return { kind: "merged", list: { ...list, items } }
}

export function useWorkersData(apiClient: ApiClient): WorkersData {
  const invalidation = useSyncExternalStore(appInvalidationStore.subscribe, appInvalidationStore.snapshot)
  const update = useSyncExternalStore(appWorkerUpdateStore.subscribe, appWorkerUpdateStore.snapshot)
  const taskUpdate = useSyncExternalStore(appTaskUpdateStore.subscribe, appTaskUpdateStore.snapshot)
  const appliedUpdateGeneration = useRef(appWorkerUpdateStore.snapshot().generation)
  const appliedTaskGeneration = useRef(appTaskUpdateStore.snapshot().generation)
  const inFlightRequest = useRef(true)
  const pendingTaskRefresh = useRef(false)
  const [workers, setWorkers] = useState<WorkerList | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [actionError, setActionError] = useState<ApiClientError | null>(null)
  const [mutating, setMutating] = useState(false)
  const [retryGeneration, setRetryGeneration] = useState(0)

  useEffect(() => {
    void invalidation.generation
    void retryGeneration
    const controller = new AbortController()
    const requestStartGeneration = appWorkerUpdateStore.snapshot().generation
    inFlightRequest.current = true
    pendingTaskRefresh.current = false
    queueMicrotask(() => {
      if (!controller.signal.aborted) {
        setLoading(true)
        setError(null)
      }
    })
    void apiClient.request("api/workers", { schema: workerListSchema, signal: controller.signal }).then(
      (nextWorkers) => {
        if (controller.signal.aborted) return
        const latestUpdate = appWorkerUpdateStore.snapshot()
        let resolvedWorkers = nextWorkers
        if (latestUpdate.generation > requestStartGeneration) {
          const { values, complete } = deltasSince(latestUpdate.recent, requestStartGeneration)
          appliedUpdateGeneration.current = latestUpdate.generation
          if (!complete) {
            setRetryGeneration((generation) => generation + 1)
            return
          }
          const reconciled = reconcileWorkerDeltas(nextWorkers, values)
          if (reconciled.kind === "refetch") {
            setRetryGeneration((generation) => generation + 1)
            return
          }
          if (reconciled.kind === "merged") {
            resolvedWorkers = reconciled.list
          }
        } else if (appliedUpdateGeneration.current < requestStartGeneration) {
          appliedUpdateGeneration.current = requestStartGeneration
        }
        inFlightRequest.current = false
        setWorkers(resolvedWorkers)
        setLoading(false)
        if (pendingTaskRefresh.current) {
          pendingTaskRefresh.current = false
          setRetryGeneration((generation) => generation + 1)
        }
      },
      (reason: unknown) => {
        if (controller.signal.aborted) return
        inFlightRequest.current = false
        pendingTaskRefresh.current = false
        if (!(reason instanceof ApiClientError)) throw reason
        setWorkers(null)
        setError(reason.code === "network_failure" ? "Controller could not be reached." : "Controller could not load workers.")
        setLoading(false)
      },
    )
    return () => controller.abort()
  }, [apiClient, invalidation.generation, retryGeneration])

  /*
   * A worker delta carries the whole authoritative DTO, so a known worker is
   * replaced in place and costs no request. A worker this page has never seen is
   * a different matter: the Controller decides list order and total, so that
   * takes one bounded refetch rather than a guessed position.
   */
  useEffect(() => {
    // Defer deltas that arrive while a list GET is in flight: when that GET
    // resolves, it replays everything since `requestStartGeneration` against the
    // returned snapshot instead of letting the response overwrite newer versions.
    if (loading || inFlightRequest.current) return
    const applied = appliedUpdateGeneration.current
    if (update.generation <= applied) return
    appliedUpdateGeneration.current = update.generation
    const { values, complete } = deltasSince(update.recent, applied)
    if (values.length === 0 && complete) return
    const refetch = () => queueMicrotask(() => setRetryGeneration((generation) => generation + 1))
    if (!complete || workers === null) {
      refetch()
      return
    }
    if (values.some((incoming) => !workers.items.some((worker) => worker.id === incoming.id))) {
      refetch()
      return
    }
    queueMicrotask(() => {
      setWorkers((previous) => {
        if (previous === null) return previous
        const reconciled = reconcileWorkerDeltas(previous, values)
        return reconciled.kind === "merged" ? reconciled.list : previous
      })
    })
  }, [loading, update.generation, update.recent, workers])

  /*
   * Slot usage is not on the worker row. `WorkerCapacity` is derived from the
   * tasks assigned to a worker, so a task moving through its lifecycle changes
   * this table without changing any worker's version -- and no worker delta is
   * published for it. A task delta therefore takes one bounded list read.
   *
   * Keep at most one refresh in flight and coalesce bursts into a single
   * trailing read instead of aborting an in-flight GET on every progress tick,
   * so continuous multi-worker task updates cannot starve the table.
   */
  useEffect(() => {
    if (taskUpdate.generation <= appliedTaskGeneration.current) return
    appliedTaskGeneration.current = taskUpdate.generation
    if (taskUpdate.task === null) return
    if (inFlightRequest.current) {
      pendingTaskRefresh.current = true
      return
    }
    queueMicrotask(() => setRetryGeneration((generation) => generation + 1))
  }, [taskUpdate.generation, taskUpdate.task])

  async function mutate(request: () => Promise<unknown>): Promise<boolean> {
    setMutating(true)
    setActionError(null)
    try {
      await request()
      setRetryGeneration((generation) => generation + 1)
      return true
    } catch (reason) {
      if (!(reason instanceof ApiClientError)) throw reason
      setActionError(reason)
      if (reason.code === "conflict") setRetryGeneration((generation) => generation + 1)
      return false
    } finally {
      setMutating(false)
    }
  }

  return {
    workers,
    loading,
    error,
    actionError,
    mutating,
    retry: () => setRetryGeneration((generation) => generation + 1),
    clearActionError: () => setActionError(null),
    createWorker: (request) => mutate(() => apiClient.request("api/workers", { method: "POST", json: request, schema: workerSchema })),
    updateWorker: (worker, request) => mutate(() => apiClient.request(`api/workers/${worker.id}`, { method: "PUT", json: request, schema: workerSchema })),
    setEnabled: (worker, enabled) => mutate(() => apiClient.request(`api/workers/${worker.id}/${enabled ? "enable" : "disable"}`, {
      method: "POST",
      json: { version: worker.version },
      schema: workerSchema,
    })),
    deleteWorker: (worker) => mutate(() => apiClient.request(`api/workers/${worker.id}?version=${worker.version}`, { method: "DELETE", schema: workerDeleteResponseSchema })),
  }
}
