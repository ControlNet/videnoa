import { Columns3, Search } from "lucide-react"

import { failureStageSchema, taskSourceSchema, taskStatusSchema } from "../api/taskSchemas"
import type { OptionalColumn, TaskLimit, TaskQuery } from "./query"
import {
  optionalColumnLabels,
  optionalColumns,
  taskLimits,
  taskOrders,
  taskSorts,
} from "./query"

type TaskToolbarProps = {
  readonly query: TaskQuery
  readonly search: string
  readonly onQueryChange: (patch: Partial<TaskQuery>) => void
  readonly onSearchChange: (value: string) => void
}

export function TaskToolbar({ query, search, onQueryChange, onSearchChange }: TaskToolbarProps) {
  return (
    <fieldset className="task-toolbar">
      <legend>Task filters</legend>
      <label className="task-search">
        <span>Search task paths</span>
        <span className="task-search-frame">
          <Search size={15} strokeWidth={1.75} aria-hidden="true" />
          <input name="search" autoComplete="off" spellCheck={false} value={search} onChange={(event) => onSearchChange(event.currentTarget.value)} />
        </span>
      </label>
      <Filter label="Status">
        <select aria-label="Status" value={query.status} onChange={(event) => onQueryChange({ status: parseStatus(event.currentTarget.value), offset: 0 })}>
          <option value="all">All statuses</option>
          {taskStatusSchema.options.map((status) => <option key={status} value={status}>{status.replaceAll("_", " ")}</option>)}
        </select>
      </Filter>
      <Filter label="Source">
        <select aria-label="Source" value={query.source} onChange={(event) => onQueryChange({ source: parseSource(event.currentTarget.value), offset: 0 })}>
          <option value="all">All sources</option>
          {taskSourceSchema.options.map((source) => <option key={source} value={source}>{source === "api" ? "API" : "Manual"}</option>)}
        </select>
      </Filter>
      <Filter label="Failure Stage">
        <select aria-label="Failure Stage" value={query.failureStage} onChange={(event) => onQueryChange({ failureStage: parseFailureStage(event.currentTarget.value), offset: 0 })}>
          <option value="all">All failure stages</option>
          {failureStageSchema.options.map((stage) => <option key={stage} value={stage}>{stage.replaceAll("_", " ")}</option>)}
        </select>
      </Filter>
      <TextFilter name="workflow" label="Workflow" value={query.workflow} onChange={(workflow) => onQueryChange({ workflow, offset: 0 })} />
      <TextFilter name="worker" label="Worker ID" value={query.worker} onChange={(worker) => onQueryChange({ worker, offset: 0 })} />
      <Filter label="Sort">
        <select aria-label="Sort" value={query.sort} onChange={(event) => onQueryChange({ sort: parseSort(event.currentTarget.value), offset: 0 })}>
          {taskSorts.map((sort) => <option key={sort} value={sort}>{sort.replaceAll("_", " ")}</option>)}
        </select>
      </Filter>
      <Filter label="Order">
        <select aria-label="Order" value={query.order} onChange={(event) => onQueryChange({ order: parseOrder(event.currentTarget.value), offset: 0 })}>
          {taskOrders.map((order) => <option key={order} value={order}>{order}</option>)}
        </select>
      </Filter>
      <Filter label="Rows">
        <select aria-label="Rows" value={query.limit} onChange={(event) => onQueryChange({ limit: parseLimit(event.currentTarget.value), offset: 0 })}>
          {taskLimits.map((limit) => <option key={limit} value={limit}>{limit}</option>)}
        </select>
      </Filter>
      <details className="column-picker">
        <summary><Columns3 size={15} strokeWidth={1.75} aria-hidden="true" /> Columns</summary>
        <div>
          {optionalColumns.map((column) => (
            <label key={column}>
              <input
                type="checkbox"
                aria-label={`Show ${optionalColumnLabels[column]} column`}
                checked={query.columns.includes(column)}
                onChange={() => onQueryChange({ columns: toggleColumn(query.columns, column) })}
              />
              {optionalColumnLabels[column]}
            </label>
          ))}
        </div>
      </details>
    </fieldset>
  )
}

function Filter({ label, children }: { readonly label: string; readonly children: React.ReactNode }) {
  return <div className="task-filter"><span>{label}</span>{children}</div>
}

function TextFilter({ name, label, value, onChange }: { readonly name: string; readonly label: string; readonly value: string; readonly onChange: (value: string) => void }) {
  return <label className="task-filter"><span>{label}</span><input name={name} autoComplete="off" spellCheck={false} value={value} onChange={(event) => onChange(event.currentTarget.value)} /></label>
}

function parseStatus(value: string): TaskQuery["status"] {
  if (value === "all") return value
  return taskStatusSchema.parse(value)
}

function parseSource(value: string): TaskQuery["source"] {
  if (value === "all") return value
  return taskSourceSchema.parse(value)
}

function parseFailureStage(value: string): TaskQuery["failureStage"] {
  if (value === "all") return value
  return failureStageSchema.parse(value)
}

function parseSort(value: string): TaskQuery["sort"] {
  return taskSorts.find((sort) => sort === value) ?? "priority"
}

function parseOrder(value: string): TaskQuery["order"] {
  return taskOrders.find((order) => order === value) ?? "desc"
}

function parseLimit(value: string): TaskLimit {
  const numeric = Number(value)
  return taskLimits.find((limit) => limit === numeric) ?? 50
}

function toggleColumn(columns: readonly OptionalColumn[], column: OptionalColumn): readonly OptionalColumn[] {
  return columns.includes(column) ? columns.filter((value) => value !== column) : [...columns, column]
}
