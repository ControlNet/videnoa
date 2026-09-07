import { useCallback, useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore } from "react"

import { type ApiClient, ApiClientError } from "../api/client"
import { type TaskDetail, taskDetailSchema } from "../api/taskSchemas"
import { appTaskUpdateStore } from "../events/taskUpdates"

type DetailOwner = {
  readonly taskId: string
  readonly generation: number
}

type HistoryRequest = {
  readonly controller: AbortController
  readonly owner: DetailOwner
  readonly offset: number
}

export type TaskDetailData = {
  readonly detail: TaskDetail | null
  readonly error: string | null
  readonly loading: boolean
  readonly loadingMore: boolean
  readonly loadMore: () => void
  readonly reload: () => void
}

export function useTaskDetail(apiClient: ApiClient, taskId: string): TaskDetailData {
  const update = useSyncExternalStore(appTaskUpdateStore.subscribe, appTaskUpdateStore.snapshot)
  const appliedGeneration = useRef(appTaskUpdateStore.snapshot().generation)
  const [detail, setDetail] = useState<TaskDetail | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [generation, setGeneration] = useState(0)
  const [loading, setLoading] = useState(true)
  const [loadingMore, setLoadingMore] = useState(false)
  const selectedTaskId = useRef(taskId)
  const generationRef = useRef(generation)
  const detailRef = useRef<TaskDetail | null>(null)
  const detailOwnerRef = useRef<DetailOwner | null>(null)
  const historyRequestRef = useRef<HistoryRequest | null>(null)

  useLayoutEffect(() => {
    selectedTaskId.current = taskId
  }, [taskId])

  const abortHistoryRequest = useCallback(() => {
    historyRequestRef.current?.controller.abort()
    historyRequestRef.current = null
  }, [])
  const cancelHistoryRequest = useCallback(() => {
    abortHistoryRequest()
    setLoadingMore(false)
  }, [abortHistoryRequest])
  const ownsHistoryRequest = useCallback((request: HistoryRequest): boolean => {
    return historyRequestRef.current === request
      && !request.controller.signal.aborted
      && selectedTaskId.current === request.owner.taskId
      && generationRef.current === request.owner.generation
  }, [])
  const reload = useCallback(() => {
    cancelHistoryRequest()
    generationRef.current += 1
    setGeneration(generationRef.current)
  }, [cancelHistoryRequest])
  const loadMore = useCallback(() => {
    const current = detailRef.current
    const owner = detailOwnerRef.current
    if (current === null || owner === null || selectedTaskId.current !== owner.taskId || generationRef.current !== owner.generation) return
    const offset = current.attempts.length
    if (historyRequestRef.current !== null || offset >= current.total) return
    const request = { controller: new AbortController(), owner, offset } satisfies HistoryRequest
    historyRequestRef.current = request
    setLoadingMore(true)
    void apiClient.request(`api/tasks/${owner.taskId}?limit=100&offset=${offset}`, { schema: taskDetailSchema, signal: request.controller.signal }).then(
      (value) => {
        if (!ownsHistoryRequest(request) || value.task.id !== owner.taskId || value.offset !== offset) return
        const activeDetail = detailRef.current
        if (activeDetail === null || activeDetail.task.id !== owner.taskId || activeDetail.attempts.length !== offset) return
        const attemptIds = new Set(activeDetail.attempts.map((attempt) => attempt.id))
        const appended = value.attempts.filter((attempt) => !attemptIds.has(attempt.id))
        const attempts = [...activeDetail.attempts, ...appended].slice(0, value.total)
        const next = {
          ...activeDetail,
          attempts,
          total: value.total,
          limit: attempts.length,
          offset: 0,
        }
        detailRef.current = next
        setDetail(next)
        setError(null)
        historyRequestRef.current = null
        setLoadingMore(false)
      },
      (reason: unknown) => {
        if (!ownsHistoryRequest(request)) return
        historyRequestRef.current = null
        setLoadingMore(false)
        if (reason instanceof ApiClientError) {
          setError(reason.code === "network_failure" ? "Controller could not load more attempts." : reason.message)
          return
        }
        throw reason
      },
    )
  }, [apiClient, ownsHistoryRequest])

  useEffect(() => {
    const owner = { taskId, generation } satisfies DetailOwner
    const controller = new AbortController()
    detailRef.current = null
    detailOwnerRef.current = null
    abortHistoryRequest()
    queueMicrotask(() => {
      if (!controller.signal.aborted && selectedTaskId.current === owner.taskId && generationRef.current === owner.generation) {
        setDetail(null)
        setError(null)
        setLoading(true)
        setLoadingMore(false)
      }
    })
    void apiClient.request(`api/tasks/${owner.taskId}?limit=100&offset=0`, { schema: taskDetailSchema, signal: controller.signal }).then(
      (value) => {
        if (controller.signal.aborted || selectedTaskId.current !== owner.taskId || generationRef.current !== owner.generation) return
        detailRef.current = value
        detailOwnerRef.current = owner
        setDetail(value)
        setLoading(false)
      },
      (reason: unknown) => {
        if (controller.signal.aborted || selectedTaskId.current !== owner.taskId || generationRef.current !== owner.generation) return
        if (reason instanceof ApiClientError) {
          setError(reason.code === "network_failure" ? "Controller could not load task detail." : reason.message)
          setLoading(false)
          return
        }
        throw reason
      },
    )
    return () => {
      controller.abort()
      abortHistoryRequest()
    }
  }, [abortHistoryRequest, apiClient, generation, taskId])

  useEffect(() => {
    if (update.generation <= appliedGeneration.current) return
    appliedGeneration.current = update.generation
    if (update.task?.id === taskId) queueMicrotask(reload)
  }, [reload, taskId, update.generation, update.task])

  return { detail, error, loading, loadingMore, loadMore, reload }
}
