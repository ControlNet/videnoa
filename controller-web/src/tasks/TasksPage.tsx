import { Layers, Plus } from "lucide-react"
import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import { useSearchParams } from "react-router"

import type { ApiClient } from "../api/client"
import { Button } from "../ui/Button"
import { useWorkerNames } from "../workers/useWorkerNames"
import "./task-actions.css"
import "./task-detail.css"
import "./tasks.css"
import { BatchTaskDialog } from "./BatchTaskDialog"
import { ManualTaskDialog } from "./ManualTaskDialog"
import { canonicalLastOffset, parseTaskQuery, serializeTaskQuery, type TaskQuery } from "./query"
import { TaskCounters } from "./TaskCounters"
import { TaskDetailPane } from "./TaskDetailPane"
import { TaskTable } from "./TaskTable"
import { TaskToolbar } from "./TaskToolbar"
import { useTasksData } from "./useTasksData"

type TasksPageProps = {
  readonly apiClient: ApiClient
}

export function TasksPage({ apiClient }: TasksPageProps) {
  const [parameters, setParameters] = useSearchParams()
  const query = useMemo(() => parseTaskQuery(parameters), [parameters])
  const [search, setSearch] = useState(query.search)
  const [addTaskOpen, setAddTaskOpen] = useState(false)
  const [batchOpen, setBatchOpen] = useState(false)
  const batchButtonRef = useRef<HTMLButtonElement>(null)
  const [selectedTaskId, setSelectedTaskId] = useState<string | null>(null)
  const addTaskButtonRef = useRef<HTMLButtonElement>(null)
  const data = useTasksData(apiClient, query)
  const workerNames = useWorkerNames(apiClient)

  useEffect(() => {
    const timer = window.setTimeout(() => setSearch(query.search), 0)
    return () => window.clearTimeout(timer)
  }, [query.search])
  const updateQuery = useCallback(
    (patch: Partial<TaskQuery>): void => {
      setParameters(serializeTaskQuery({ ...query, ...patch }), {
        replace: true,
      })
    },
    [query, setParameters],
  )

  useEffect(() => {
    if (search === query.search) return
    const timer = window.setTimeout(() => updateQuery({ search, offset: 0 }), 300)
    return () => window.clearTimeout(timer)
  }, [query.search, search, updateQuery])

  useEffect(() => {
    if (data.page === null || data.page.items.length > 0 || query.offset === 0) return
    if (data.page.offset !== query.offset || data.page.limit !== query.limit) return
    const offset = canonicalLastOffset(data.page.total, query.limit)
    if (offset < query.offset) updateQuery({ offset })
  }, [data.page, query.limit, query.offset, updateQuery])

  const page = data.page
  const hasPageRange = page !== null && page.items.length > 0 && page.offset === query.offset && page.limit === query.limit && page.offset < page.total
  const first = hasPageRange ? page.offset + 1 : 0
  const last = hasPageRange ? Math.min(page.offset + page.items.length, page.total) : 0

  function closeAddTask(): void {
    setAddTaskOpen(false)
    queueMicrotask(() => addTaskButtonRef.current?.focus())
  }

  function closeDetail(): void {
    const selected = selectedTaskId
    setSelectedTaskId(null)
    queueMicrotask(() => {
      const rowButton = selected === null ? null : document.querySelector<HTMLButtonElement>(`[data-task-id="${selected}"]`)
      ;(rowButton ?? addTaskButtonRef.current)?.focus()
    })
  }

  return (
    <div className="route-page tasks-page">
      <TaskToolbar
        query={query}
        search={search}
        workerNames={workerNames}
        onQueryChange={updateQuery}
        onSearchChange={setSearch}
        heading={<h1>Tasks</h1>}
        counters={<TaskCounters counts={data.counts} />}
        actions={<>
          <Button ref={batchButtonRef} variant="outline" onClick={() => setBatchOpen(true)}>
            <Layers size={13} strokeWidth={2.4} aria-hidden="true" />Add Batch
          </Button>
          <Button ref={addTaskButtonRef} variant="primary" onClick={() => setAddTaskOpen(true)}>
            <Plus size={13} strokeWidth={2.4} aria-hidden="true" />
            Add Task
          </Button>
        </>}
      />
      {data.error === null ? null : (
        <div className="task-load-error alert alert--danger" role="alert">
          <span>{data.error}</span>
          <button type="button" onClick={data.retry}>
            Retry
          </button>
        </div>
      )}
      <div className="task-workspace">
        <div className="task-table-region">
          <TaskTable page={data.page} columns={query.columns} loading={data.loading} workerNames={workerNames} selectedTaskId={selectedTaskId} onSelectTask={setSelectedTaskId} />
        </div>
        {selectedTaskId === null ? null : <TaskDetailPane apiClient={apiClient} taskId={selectedTaskId} onClose={closeDetail} onChanged={data.retry} />}
      </div>
      <footer className="task-pagination">
        <span>
          {first.toLocaleString()}-{last.toLocaleString()} of {page?.total.toLocaleString() ?? "--"}
        </span>
        <div>
          <button type="button" disabled={query.offset === 0 || data.loading} onClick={() => updateQuery({ offset: Math.max(0, query.offset - query.limit) })}>
            Previous
          </button>
          <button
            type="button"
            disabled={!hasPageRange || last >= page.total || data.loading}
            onClick={() => updateQuery({ offset: query.offset + query.limit })}
          >
            Next
          </button>
        </div>
      </footer>
      {batchOpen ? <BatchTaskDialog apiClient={apiClient} onCreated={data.retry} onClose={() => {
        setBatchOpen(false)
        queueMicrotask(() => batchButtonRef.current?.focus())
      }} /> : null}
      <ManualTaskDialog
        apiClient={apiClient}
        open={addTaskOpen}
        onClose={closeAddTask}
        onCreated={(task) => {
          setSelectedTaskId(task.id)
          data.retry()
        }}
      />
    </div>
  )
}
