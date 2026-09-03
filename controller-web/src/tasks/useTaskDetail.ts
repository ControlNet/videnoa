import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from "react"

import { type ApiClient, ApiClientError } from "../api/client"
import { type TaskDetail, taskDetailSchema } from "../api/taskSchemas"
import { appTaskUpdateStore } from "../events/taskUpdates"

export type TaskDetailData = {
  readonly detail: TaskDetail | null
  readonly error: string | null
  readonly loading: boolean
  readonly reload: () => void
}

export function useTaskDetail(apiClient: ApiClient, taskId: string): TaskDetailData {
  const update = useSyncExternalStore(appTaskUpdateStore.subscribe, appTaskUpdateStore.snapshot)
  const appliedGeneration = useRef(appTaskUpdateStore.snapshot().generation)
  const [detail, setDetail] = useState<TaskDetail | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [generation, setGeneration] = useState(0)
  const [loading, setLoading] = useState(true)
  const reload = useCallback(() => setGeneration((value) => value + 1), [])

  useEffect(() => {
    void generation
    const controller = new AbortController()
    queueMicrotask(() => {
      if (!controller.signal.aborted) {
        setDetail(null)
        setError(null)
        setLoading(true)
      }
    })
    void apiClient.request(`api/tasks/${taskId}`, { schema: taskDetailSchema, signal: controller.signal }).then(
      (value) => {
        if (controller.signal.aborted) return
        setDetail(value)
        setLoading(false)
      },
      (reason: unknown) => {
        if (controller.signal.aborted) return
        if (reason instanceof ApiClientError) {
          setError(reason.code === "network_failure" ? "Controller could not load task detail." : reason.message)
          setLoading(false)
          return
        }
        throw reason
      },
    )
    return () => controller.abort()
  }, [apiClient, generation, taskId])

  useEffect(() => {
    if (update.generation <= appliedGeneration.current) return
    appliedGeneration.current = update.generation
    if (update.task?.id === taskId) queueMicrotask(reload)
  }, [reload, taskId, update.generation, update.task])

  return { detail, error, loading, reload }
}
