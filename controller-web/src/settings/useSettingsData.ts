import { useEffect, useRef, useState, useSyncExternalStore } from "react"

import { type ApiClient, ApiClientError } from "../api/client"
import {
  type Readiness,
  readinessSchema,
  type SettingsResponse,
  type SettingsUpdateRequest,
  settingsResponseSchema,
} from "../api/settingsSchemas"
import { appSchedulerUpdateStore } from "../events/schedulerUpdates"
import { appInvalidationStore } from "../events/store"

export type SettingsData = {
  readonly settings: SettingsResponse | null
  readonly readiness: Readiness | null
  readonly loading: boolean
  readonly error: string | null
  readonly actionError: ApiClientError | null
  readonly mutating: boolean
  readonly retry: () => void
  readonly clearActionError: () => void
  readonly save: (request: SettingsUpdateRequest) => Promise<SettingsMutationResult>
  readonly setPaused: (paused: boolean) => Promise<boolean>
}

export type SettingsMutationResult =
  | { readonly ok: true; readonly settings: SettingsResponse }
  | { readonly ok: false; readonly error: ApiClientError }

export function useSettingsData(apiClient: ApiClient): SettingsData {
  const invalidation = useSyncExternalStore(appInvalidationStore.subscribe, appInvalidationStore.snapshot)
  const update = useSyncExternalStore(appSchedulerUpdateStore.subscribe, appSchedulerUpdateStore.snapshot)
  const appliedUpdateGeneration = useRef(appSchedulerUpdateStore.snapshot().generation)
  const [settings, setSettings] = useState<SettingsResponse | null>(null)
  const [readiness, setReadiness] = useState<Readiness | null>(null)
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
    void Promise.all([
      apiClient.request("api/settings", { schema: settingsResponseSchema, signal: controller.signal }),
      apiClient.request("api/readiness", { schema: readinessSchema, signal: controller.signal }),
    ]).then(
      ([nextSettings, nextReadiness]) => {
        if (controller.signal.aborted) return
        setSettings(nextSettings)
        setReadiness(nextReadiness)
        setLoading(false)
      },
      (reason: unknown) => {
        if (controller.signal.aborted) return
        if (!(reason instanceof ApiClientError)) throw reason
        // Keep any mounted draft available while the authoritative refresh is retried.
        setReadiness(null)
        setError(reason.code === "network_failure" ? "Controller could not be reached." : "Controller could not load settings.")
        setLoading(false)
      },
    )
    return () => controller.abort()
  }, [apiClient, invalidation.generation, retryGeneration])

  /*
   * A scheduler delta is shown at once and then reconciled by one bounded read.
   *
   * Both halves are needed. The delta carries the new scheduler status but no
   * settings version, and every mutation on this route sends one, so keeping
   * only the merged value would make the next save or pause answer 409. The
   * read supplies the coherent version. The editor reconciles untouched fields
   * while retaining unsaved edits and notifying the operator of remote changes.
   */
  useEffect(() => {
    if (update.generation <= appliedUpdateGeneration.current) return
    appliedUpdateGeneration.current = update.generation
    const incoming = update.scheduler
    if (incoming === null) return
    queueMicrotask(() => {
      setSettings((current) => (current === null ? current : { ...current, scheduler: incoming }))
      setRetryGeneration((generation) => generation + 1)
    })
  }, [update.generation, update.scheduler])

  async function mutate(request: () => Promise<SettingsResponse>): Promise<SettingsMutationResult> {
    setMutating(true)
    setActionError(null)
    try {
      const nextSettings = await request()
      setSettings(nextSettings)
      return { ok: true, settings: nextSettings }
    } catch (reason) {
      if (!(reason instanceof ApiClientError)) throw reason
      setActionError(reason)
      if (reason.code === "conflict" || (reason.code === "unavailable" && reason.retryable)) {
        setLoading(true)
        setRetryGeneration((generation) => generation + 1)
      }
      return { ok: false, error: reason }
    } finally {
      setMutating(false)
    }
  }

  return {
    settings,
    readiness,
    loading,
    error,
    actionError,
    mutating,
    retry: () => setRetryGeneration((generation) => generation + 1),
    clearActionError: () => setActionError(null),
    save: (request) => mutate(() => apiClient.request("api/settings", { method: "PUT", json: request, schema: settingsResponseSchema })),
    setPaused: async (paused) => {
      if (settings === null) return Promise.resolve(false)
      const result = await mutate(() => apiClient.request(`api/scheduler/${paused ? "pause" : "resume"}`, {
        method: "POST",
        json: { version: settings.version },
        schema: settingsResponseSchema,
      }))
      return result.ok
    },
  }
}
