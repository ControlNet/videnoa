import { Columns3, Search } from "lucide-react"
import type { ReactNode } from "react"

import { failureStageSchema, taskSourceSchema, taskStatusSchema } from "../api/taskSchemas"
import { SelectChip, TextChip } from "../ui/Chip"
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
  /** Registered worker names by identifier, so the filter offers what the table shows. */
  readonly workerNames: ReadonlyMap<string, string>
  readonly onQueryChange: (patch: Partial<TaskQuery>) => void
  readonly onSearchChange: (value: string) => void
  /**
   * The toolbar owns both control bands so the route heading, search, column
   * picker and primary action share one 48px command row above the filters.
   */
  readonly heading?: ReactNode
  readonly counters?: ReactNode
  readonly actions?: ReactNode
}

export function TaskToolbar({ query, search, workerNames, onQueryChange, onSearchChange, heading, counters, actions }: TaskToolbarProps) {
  /*
   * The worker column shows names, so this filter must offer names -- typing an
   * identifier was the only way to use it before, which contradicted everything
   * on screen. The value stays the identifier because that is what the
   * Controller filters on. A selected worker that is no longer registered keeps
   * its own option, or choosing it back would silently widen the filter.
   */
  const workerOptions = [...workerNames].sort(([, left], [, right]) => left.localeCompare(right))
  const selectedWorkerIsListed = query.worker === "" || workerNames.has(query.worker)
  return (
    <>
      <div className="command-row">
        {heading}
        <span className="spacer" />
        <span className="input-frame task-search">
          <Search size={13} strokeWidth={2} aria-hidden="true" />
          <input
            name="search"
            aria-label="Search task paths"
            placeholder="Search input or output path"
            autoComplete="off"
            spellCheck={false}
            value={search}
            onChange={(event) => onSearchChange(event.currentTarget.value)}
          />
        </span>
        <details className="column-picker">
          <summary><Columns3 size={13} strokeWidth={1.8} aria-hidden="true" /> Columns</summary>
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
        {actions}
      </div>

      <fieldset className="task-toolbar">
        <legend className="sr-only">Task filters</legend>
        {counters}
        <span className="spacer" />
        <div className="task-filter-chips">
          <SelectChip label="Status" value={query.status} neutralValue="all" onChange={(value) => onQueryChange({ status: parseStatus(value), offset: 0 })}>
            <option value="all">Any</option>
            {taskStatusSchema.options.map((status) => <option key={status} value={status}>{status.replaceAll("_", " ")}</option>)}
          </SelectChip>
          <SelectChip label="Source" value={query.source} neutralValue="all" onChange={(value) => onQueryChange({ source: parseSource(value), offset: 0 })}>
            <option value="all">Any</option>
            {taskSourceSchema.options.map((source) => <option key={source} value={source}>{source === "api" ? "API" : "Manual"}</option>)}
          </SelectChip>
          <SelectChip label="Failure Stage" shortLabel="Stage" value={query.failureStage} neutralValue="all" onChange={(value) => onQueryChange({ failureStage: parseFailureStage(value), offset: 0 })}>
            <option value="all">Any</option>
            {failureStageSchema.options.map((stage) => <option key={stage} value={stage}>{stage.replaceAll("_", " ")}</option>)}
          </SelectChip>
          <TextChip name="workflow" label="Workflow" placeholder="Any" value={query.workflow} onChange={(workflow) => onQueryChange({ workflow, offset: 0 })} />
          <SelectChip label="Worker" value={query.worker} neutralValue="" onChange={(worker) => onQueryChange({ worker, offset: 0 })}>
            <option value="">Any</option>
            {workerOptions.map(([id, name]) => <option key={id} value={id}>{name}</option>)}
            {selectedWorkerIsListed ? null : <option value={query.worker}>{shortWorkerId(query.worker)}</option>}
          </SelectChip>
          <span className="chip-divider" aria-hidden="true" />
          <SelectChip label="Sort" value={query.sort} neutralValue="priority" onChange={(value) => onQueryChange({ sort: parseSort(value), offset: 0 })}>
            {taskSorts.map((sort) => <option key={sort} value={sort}>{sort.replaceAll("_", " ")}</option>)}
          </SelectChip>
          <SelectChip label="Order" value={query.order} neutralValue="desc" onChange={(value) => onQueryChange({ order: parseOrder(value), offset: 0 })}>
            {taskOrders.map((order) => <option key={order} value={order}>{order}</option>)}
          </SelectChip>
          <SelectChip label="Rows" value={String(query.limit)} neutralValue="50" onChange={(value) => onQueryChange({ limit: parseLimit(value), offset: 0 })}>
            {taskLimits.map((limit) => <option key={limit} value={limit}>{limit}</option>)}
          </SelectChip>
        </div>
      </fieldset>
    </>
  )
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

/** A deregistered worker still selected in the filter: identify it, do not pretend to name it. */
function shortWorkerId(id: string): string {
  return `${id.slice(0, 8)} (unregistered)`
}
