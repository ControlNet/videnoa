import { useCallback, useEffect, useMemo, useState } from "react"
import { useSearchParams } from "react-router"

import type { ApiClient } from "../api/client"
import type { TaskQuery } from "./query"
import { canonicalLastOffset, parseTaskQuery, serializeTaskQuery } from "./query"
import { TaskCounters } from "./TaskCounters"
import { TaskTable } from "./TaskTable"
import { TaskToolbar } from "./TaskToolbar"
import { useTasksData } from "./useTasksData"
import "./tasks.css"

type TasksPageProps = {
  readonly apiClient: ApiClient
}

export function TasksPage({ apiClient }: TasksPageProps) {
  const [parameters, setParameters] = useSearchParams()
  const query = useMemo(() => parseTaskQuery(parameters), [parameters])
  const [search, setSearch] = useState(query.search)
  const data = useTasksData(apiClient, query)

  useEffect(() => {
    const timer = window.setTimeout(() => setSearch(query.search), 0)
    return () => window.clearTimeout(timer)
  }, [query.search])
  const updateQuery = useCallback((patch: Partial<TaskQuery>): void => {
    setParameters(serializeTaskQuery({ ...query, ...patch }), { replace: true })
  }, [query, setParameters])

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
  const hasPageRange = page !== null
    && page.items.length > 0
    && page.offset === query.offset
    && page.limit === query.limit
    && page.offset < page.total
  const first = hasPageRange ? page.offset + 1 : 0
  const last = hasPageRange ? Math.min(page.offset + page.items.length, page.total) : 0

  return (
    <div className="route-page tasks-page">
      <header className="tasks-header">
        <div><p className="technical-label">DURABLE WORK HISTORY</p><h1>Tasks</h1><p>Monitor bounded task history and active processing state from the Controller.</p></div>
        <TaskCounters counts={data.counts} />
      </header>
      <TaskToolbar query={query} search={search} onQueryChange={updateQuery} onSearchChange={setSearch} />
      {data.error === null ? null : <div className="task-load-error" role="alert"><span>{data.error}</span><button type="button" onClick={data.retry}>Retry</button></div>}
      <TaskTable page={data.page} columns={query.columns} loading={data.loading} />
      <footer className="task-pagination">
        <span>{first.toLocaleString()}-{last.toLocaleString()} of {page?.total.toLocaleString() ?? "--"}</span>
        <div>
          <button type="button" disabled={query.offset === 0 || data.loading} onClick={() => updateQuery({ offset: Math.max(0, query.offset - query.limit) })}>Previous</button>
          <button type="button" disabled={!hasPageRange || last >= page.total || data.loading} onClick={() => updateQuery({ offset: query.offset + query.limit })}>Next</button>
        </div>
      </footer>
    </div>
  )
}
