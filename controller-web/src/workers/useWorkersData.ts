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

export function useWorkersData(apiClient: ApiClient): WorkersData {
  const invalidation = useSyncExternalStore(appInvalidationStore.subscribe, appInvalidationStore.snapshot)
  const update = useSyncExternalStore(appWorkerUpdateStore.subscribe, appWorkerUpdateStore.snapshot)
  const taskUpdate = useSyncExternalStore(appTaskUpdateStore.subscribe, appTaskUpdateStore.snapshot)
  const appliedUpdateGeneration = useRef(appWorkerUpdateStore.snapshot().generation)
  const appliedTaskGeneration = useRef(appTaskUpdateStore.snapshot().generation)
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
    queueMicrotask(() => {
      if (!controller.signal.aborted) {
        setLoading(true)
        setError(null)
      }
    })
    void apiClient.request("api/workers", { schema: workerListSchema, signal: controller.signal }).then(
      (nextWorkers) => {
        if (controller.signal.aborted) return
        setWorkers(nextWorkers)
        setLoading(false)
      },
      (reason: unknown) => {
        if (controller.signal.aborted) return
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
    if (update.generation <= appliedUpdateGeneration.current) return
    appliedUpdateGeneration.current = update.generation
    const incoming = update.worker
    if (incoming === null) return
    if (workers === null || loading) {
      queueMicrotask(() => setRetryGeneration((generation) => generation + 1))
      return
    }
    const current = workers.items.find((worker) => worker.id === incoming.id)
    queueMicrotask(() => {
      if (current === undefined) {
        setRetryGeneration((generation) => generation + 1)
        return
      }
      if (incoming.version <= current.version) return
      setWorkers({
        ...workers,
        items: workers.items.map((worker) => (worker.id === incoming.id ? incoming : worker)),
      })
    })
  }, [loading, update.generation, update.worker, workers])

  /*
   * Slot usage is not on the worker row. `WorkerCapacity` is derived from the
   * tasks assigned to a worker, so a task moving through its lifecycle changes
   * this table without changing any worker's version -- and no worker delta is
   * published for it. A task delta therefore takes one bounded list read.
   *
   * That costs one request per task event while this route is mounted, roughly
   * one a second per actively processing task. It is accepted deliberately:
   * recomputing capacity here would duplicate the Controller's own counting,
   * and an operator watching this table is watching it for these numbers. The
   * read keeps the rendered list, so nothing blanks while it is in flight.
   */
  useEffect(() => {
    if (taskUpdate.generation <= appliedTaskGeneration.current) return
    appliedTaskGeneration.current = taskUpdate.generation
    if (taskUpdate.task === null) return
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
