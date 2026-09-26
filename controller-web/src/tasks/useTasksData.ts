import { useEffect, useRef, useState, useSyncExternalStore } from "react"

import { type ApiClient, ApiClientError } from "../api/client"
import {
  type TaskList,
  type TaskStatusCounts,
  taskListSchema,
  taskStatusCountsSchema,
} from "../api/taskSchemas"
import { appInvalidationStore } from "../events/store"
import { appTaskUpdateStore } from "../events/taskUpdates"
import { canMergeTaskUpdate, matchesTaskQuery } from "./model"
import { type TaskQuery, taskPagePath } from "./query"

export type TasksData = {
  readonly page: TaskList | null
  readonly counts: TaskStatusCounts | null
  readonly error: string | null
  readonly loading: boolean
  readonly retry: () => void
}

type LoadedTaskPage = {
  readonly path: string
  readonly page: TaskList
}

export function useTasksData(apiClient: ApiClient, query: TaskQuery): TasksData {
  const invalidation = useSyncExternalStore(appInvalidationStore.subscribe, appInvalidationStore.snapshot)
  const update = useSyncExternalStore(appTaskUpdateStore.subscribe, appTaskUpdateStore.snapshot)
  const pagePath = taskPagePath(query)
  const [loadedPage, setLoadedPage] = useState<LoadedTaskPage | null>(null)
  const [counts, setCounts] = useState<TaskStatusCounts | null>(null)
  const [countsError, setCountsError] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [retryGeneration, setRetryGeneration] = useState(0)
  const [countsRetryGeneration, setCountsRetryGeneration] = useState(0)
  const appliedUpdateGeneration = useRef(appTaskUpdateStore.snapshot().generation)

  useEffect(() => {
    void invalidation.generation
    void retryGeneration
    const controller = new AbortController()
    /*
     * The rendered page is deliberately left in place while this read is in
     * flight.
     *
     * `page` below already resolves to null whenever the loaded path is not the
     * requested one, so clearing it was redundant for a query change -- and for
     * a refresh of the same query it was destructive. A task status change
     * refuses the in-place merge and refetches, so clearing first tore down
     * every row, mounted loading skeletons in their place, and then rebuilt the
     * table: on a page of fifty rows that measured 58 rows destroyed and 116
     * childList mutations for one row changing status. Keeping the rows lets
     * React reconcile the one that actually changed.
     */
    queueMicrotask(() => {
      if (!controller.signal.aborted) {
        setLoading(true)
        setError(null)
      }
    })
    void apiClient.request(pagePath, { schema: taskListSchema, signal: controller.signal }).then(
      (nextPage) => {
        if (controller.signal.aborted) return
        setLoadedPage({ path: pagePath, page: nextPage })
        setLoading(false)
      },
      (reason: unknown) => {
        if (controller.signal.aborted) return
        if (reason instanceof ApiClientError) {
          setError(messageFor(reason))
          setLoading(false)
          return
        }
        setError("Controller could not load task history.")
        setLoading(false)
      },
    )
    return () => controller.abort()
  }, [apiClient, invalidation.generation, pagePath, retryGeneration])

  // Global counters are independent of page membership. Keep one read in flight
  // and coalesce events into a trailing refresh, even during continuous progress.
  useEffect(() => {
    void invalidation.generation
    void countsRetryGeneration
    const controller = new AbortController()
    let timer: ReturnType<typeof setTimeout> | undefined
    let loadingCounts = false
    let dirty = false

    function schedule() {
      if (loadingCounts || timer !== undefined || controller.signal.aborted) return
      timer = setTimeout(() => {
        timer = undefined
        dirty = false
        void load()
      }, 1000)
    }

    async function load() {
      loadingCounts = true
      try {
        const nextCounts = await apiClient.request("api/status-counts", { schema: taskStatusCountsSchema, signal: controller.signal })
        if (controller.signal.aborted) return
        setCounts(nextCounts)
        setCountsError(null)
      } catch (reason) {
        if (controller.signal.aborted) return
        setCountsError(reason instanceof ApiClientError ? messageFor(reason) : "Controller could not load task counts.")
      } finally {
        loadingCounts = false
        if (dirty) schedule()
      }
    }

    const unsubscribe = appTaskUpdateStore.subscribe(() => {
      dirty = true
      schedule()
    })
    void load()
    return () => { controller.abort(); unsubscribe(); clearTimeout(timer) }
  }, [apiClient, invalidation.generation, countsRetryGeneration])

  const page = loadedPage?.path === pagePath ? loadedPage.page : null

  useEffect(() => {
    if (update.generation <= appliedUpdateGeneration.current) return
    appliedUpdateGeneration.current = update.generation
    const incoming = update.task
    if (incoming === null) return
    if (page === null || loading) {
      if (matchesTaskQuery(incoming, query)) {
        queueMicrotask(() => setRetryGeneration((generation) => generation + 1))
      }
      return
    }
    const current = page.items.find((task) => task.id === incoming.id)
    queueMicrotask(() => {
      if (current === undefined) {
        if (matchesTaskQuery(incoming, query)) {
          setRetryGeneration((generation) => generation + 1)
        }
        return
      }
      if (incoming.version <= current.version) return
      if (!canMergeTaskUpdate(current, incoming, query)) {
        setRetryGeneration((generation) => generation + 1)
        return
      }
      setLoadedPage({
        path: pagePath,
        page: {
          ...page,
          items: page.items.map((task) => task.id === incoming.id ? incoming : task),
        },
      })
    })
  }, [loading, page, pagePath, query, update.generation, update.task])

  return {
    page,
    counts,
    error: error ?? countsError,
    loading,
    retry: () => {
      setRetryGeneration((generation) => generation + 1)
      setCountsRetryGeneration((generation) => generation + 1)
    },
  }
}

function messageFor(error: ApiClientError): string {
  switch (error.code) {
    case "malformed_response":
      return "Controller returned invalid task data."
    case "network_failure":
      return "Controller could not be reached."
    case "unauthorized":
      return "The Controller session expired."
    case "forbidden":
    case "rate_limited":
    case "http_error":
    case "internal":
    case "internal_error":
    case "invalid_request":
    case "not_found":
    case "conflict":
    case "publication_ambiguous":
    case "remote_state_ambiguous":
    case "unavailable":
      return "Controller could not load task history."
  }
}
