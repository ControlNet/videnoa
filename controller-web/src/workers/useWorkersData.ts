import { useEffect, useState, useSyncExternalStore } from "react"

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
