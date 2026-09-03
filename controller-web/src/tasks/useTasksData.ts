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
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [retryGeneration, setRetryGeneration] = useState(0)
  const appliedUpdateGeneration = useRef(appTaskUpdateStore.snapshot().generation)

  useEffect(() => {
    void invalidation.generation
    void retryGeneration
    const controller = new AbortController()
    queueMicrotask(() => {
      if (!controller.signal.aborted) {
        setLoadedPage(null)
        setLoading(true)
        setError(null)
      }
    })
    void Promise.all([
      apiClient.request(pagePath, { schema: taskListSchema, signal: controller.signal }),
      apiClient.request("api/status-counts", { schema: taskStatusCountsSchema, signal: controller.signal }),
    ]).then(
      ([nextPage, nextCounts]) => {
        if (controller.signal.aborted) return
        setLoadedPage({ path: pagePath, page: nextPage })
        setCounts(nextCounts)
        setLoading(false)
      },
      (reason: unknown) => {
        if (controller.signal.aborted) return
        if (reason instanceof ApiClientError) {
          setError(messageFor(reason))
          setLoading(false)
          return
        }
        throw reason
      },
    )
    return () => controller.abort()
  }, [apiClient, invalidation.generation, pagePath, retryGeneration])

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
    error,
    loading,
    retry: () => setRetryGeneration((generation) => generation + 1),
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
