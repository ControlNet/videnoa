import { useCallback, useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore } from "react"

import { type ApiClient, ApiClientError } from "../api/client"
import { type TaskDetail, taskDetailSchema } from "../api/taskSchemas"
import { deltasSince } from "../events/deltas"
import { appTaskUpdateStore } from "../events/taskUpdates"
import { appInvalidationStore } from "../events/store"

/** The Controller's default and maximum attempt page, from `PageLimit`. */
const attemptPageSize = 100
const attemptPageMaximum = 500

type DetailOwner = {
  readonly taskId: string
  readonly generation: number
}

type HistoryRequest = {
  readonly controller: AbortController
  readonly taskId: string
  readonly offset: number
  readonly headAttemptId: string | null
}

export type TaskDetailData = {
  readonly detail: TaskDetail | null
  readonly error: string | null
  readonly loading: boolean
  readonly loadingMore: boolean
  readonly loadMore: () => void
  readonly reload: () => void
}

/*
 * Restores attempts that the refreshed window could not reach.
 *
 * One request cannot exceed the Controller's page maximum, so an operator who
 * expanded beyond it would otherwise watch the list shrink on every update.
 * Attempt history is append-only and ordered newest first, which makes anything
 * behind the freshly read head immutable: carrying the entries the response did
 * not contain rebuilds the full window in order, with no second request and no
 * gap. The authoritative `total` still bounds the result.
 */
function withRetainedAttempts(value: TaskDetail, retained: TaskDetail | null): TaskDetail {
  if (retained === null || retained.task.id !== value.task.id) return value
  const refreshed = new Set(value.attempts.map((attempt) => attempt.id))
  const carried = retained.attempts.filter((attempt) => !refreshed.has(attempt.id))
  if (carried.length === 0) return value
  const attempts = [...value.attempts, ...carried].slice(0, value.total)
  return { ...value, attempts, limit: attempts.length, offset: 0 }
}

export function useTaskDetail(apiClient: ApiClient, taskId: string): TaskDetailData {
  const invalidation = useSyncExternalStore(appInvalidationStore.subscribe, appInvalidationStore.snapshot)
  const appliedInvalidation = useRef(invalidation.generation)
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
      && selectedTaskId.current === request.taskId
  }, [])
  const reload = useCallback(() => {
    cancelHistoryRequest()
    generationRef.current += 1
    setGeneration(generationRef.current)
  }, [cancelHistoryRequest])
  const refreshSameTask = useCallback(() => {
    generationRef.current += 1
    setGeneration(generationRef.current)
  }, [])
  const loadMore = useCallback(() => {
    const current = detailRef.current
    const owner = detailOwnerRef.current
    if (current === null || owner === null || selectedTaskId.current !== owner.taskId || generationRef.current !== owner.generation) return
    const offset = current.attempts.length
    if (historyRequestRef.current !== null || offset >= current.total) return
    const headAttemptId = current.attempts[0]?.id ?? null
    const request = { controller: new AbortController(), taskId: owner.taskId, offset, headAttemptId } satisfies HistoryRequest
    historyRequestRef.current = request
    setLoadingMore(true)
    void apiClient.request(`api/tasks/${request.taskId}?limit=${attemptPageSize}&offset=${offset}`, { schema: taskDetailSchema, signal: request.controller.signal }).then(
      (value) => {
        if (!ownsHistoryRequest(request)) return
        const activeDetail = detailRef.current
        if (
          value.task.id !== request.taskId
          || value.offset !== offset
          || activeDetail === null
          || activeDetail.task.id !== request.taskId
          || activeDetail.attempts.length !== offset
          || (activeDetail.attempts[0]?.id ?? null) !== request.headAttemptId
        ) {
          historyRequestRef.current = null
          setLoadingMore(false)
          return
        }
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
    return () => {
      abortHistoryRequest()
    }
  }, [abortHistoryRequest, taskId])

  useEffect(() => {
    const owner = { taskId, generation } satisfies DetailOwner
    const controller = new AbortController()
    /*
     * Selecting a different task invalidates what is on screen; refreshing the
     * one already shown does not.
     *
     * A processing task reports an update roughly every second, and clearing the
     * detail on each of those unmounted the whole inspector down to a loading
     * line. That collapsed the drawer's scroll height, so the browser clamped
     * scrollTop to zero and the operator was thrown back to the top once a
     * second. A refresh therefore swaps its content in place and lets React
     * reconcile the fields that actually changed; only a genuine selection
     * change falls back to the loading state.
     */
    const isSelectionChange = detailRef.current?.task.id !== taskId
    if (isSelectionChange) {
      detailRef.current = null
      detailOwnerRef.current = null
      abortHistoryRequest()
    }
    queueMicrotask(() => {
      if (controller.signal.aborted || selectedTaskId.current !== owner.taskId || generationRef.current !== owner.generation) return
      setError(null)
      if (isSelectionChange) {
        setLoadingMore(false)
        setDetail(null)
        setLoading(true)
      }
    })
    /*
     * A refresh re-reads the window the operator has open, not just its first
     * page: re-requesting the default page collapsed an expanded history back to
     * 100 attempts on every update.
     */
    const retainedAtStart = isSelectionChange ? null : detailRef.current
    const limit = Math.min(Math.max(retainedAtStart?.attempts.length ?? attemptPageSize, attemptPageSize), attemptPageMaximum)
    void apiClient.request(`api/tasks/${owner.taskId}?limit=${limit}&offset=0`, { schema: taskDetailSchema, signal: controller.signal }).then(
      (value) => {
        if (controller.signal.aborted || selectedTaskId.current !== owner.taskId || generationRef.current !== owner.generation) return
        const currentRetained = isSelectionChange ? null : detailRef.current
        const next = withRetainedAttempts(value, currentRetained)
        detailRef.current = next
        detailOwnerRef.current = owner
        const pendingHistory = historyRequestRef.current
        if (
          pendingHistory !== null
          && (
            pendingHistory.taskId !== next.task.id
            || next.attempts.length !== pendingHistory.offset
            || next.attempts.length >= next.total
            || (next.attempts[0]?.id ?? null) !== pendingHistory.headAttemptId
          )
        ) {
          abortHistoryRequest()
          setLoadingMore(false)
        }
        setDetail(next)
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
    }
  }, [abortHistoryRequest, apiClient, generation, taskId])

  useEffect(() => {
    if (invalidation.generation <= appliedInvalidation.current) return
    appliedInvalidation.current = invalidation.generation
    queueMicrotask(reload)
  }, [invalidation.generation, reload])

  useEffect(() => {
    const applied = appliedGeneration.current
    if (update.generation <= applied) return
    appliedGeneration.current = update.generation
    const { values, complete } = deltasSince(update.recent, applied)
    if (!complete) {
      queueMicrotask(reload)
    } else if (values.some((task) => task.id === taskId)) {
      queueMicrotask(refreshSameTask)
    }
  }, [refreshSameTask, reload, taskId, update.generation, update.recent])

  return { detail, error, loading, loadingMore, loadMore, reload }
}
