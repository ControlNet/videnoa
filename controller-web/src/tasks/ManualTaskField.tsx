import { ChevronDown, File, Folder, Workflow } from "lucide-react"
import { useEffect, useRef, useState } from "react"

import type { ApiClient } from "../api/client"
import { loadTaskSuggestions, type TaskSuggestion, type TaskSuggestionField } from "./taskSuggestions"
import "./task-suggestions.css"

type TaskFieldProps = {
  readonly apiClient: ApiClient
  readonly enabled: boolean
  readonly label: string
  readonly name: TaskSuggestionField
  readonly value: string
  readonly error: string | undefined
  readonly inputRef: React.RefObject<HTMLInputElement | null>
  readonly onChange: (value: string) => void
}

export function ManualTaskField({ apiClient, enabled, label, name, value, error, inputRef, onChange }: TaskFieldProps) {
  const id = `task-${name.replaceAll("_", "-")}`
  const errorId = `${id}-error`
  const listId = `${id}-suggestions`
  const [expanded, setExpanded] = useState(false)
  const [active, setActive] = useState(-1)
  const [result, setResult] = useState<{ value: string; items: TaskSuggestion[]; truncated: boolean; failed: boolean } | null>(null)
  const listRef = useRef<HTMLUListElement>(null)
  const open = enabled && expanded
  const current = result?.value === value ? result : null
  const items = current?.items ?? []

  useEffect(() => {
    if (!open) return
    const controller = new AbortController()
    const timer = window.setTimeout(() => {
      void loadTaskSuggestions(apiClient, name, value, controller.signal).then((next) => {
        if (!controller.signal.aborted) setResult({ value, ...next, failed: false })
      }).catch(() => {
        if (!controller.signal.aborted) setResult({ value, items: [], truncated: false, failed: true })
      })
    }, 300)
    return () => {
      window.clearTimeout(timer)
      controller.abort()
    }
  }, [apiClient, name, open, value])

  useEffect(() => {
    if (open && active >= 0) listRef.current?.children[active]?.scrollIntoView?.({ block: "nearest" })
  }, [active, open])

  function choose(item: TaskSuggestion): void {
    onChange(item.value)
    setActive(-1)
    setResult(null)
    setExpanded(item.kind === "directory")
    inputRef.current?.focus()
  }

  return (
    <div className="field task-suggestion-field" onBlur={(event) => {
      if (!event.currentTarget.contains(event.relatedTarget)) setExpanded(false)
    }}>
      <label htmlFor={id}>{label}</label>
      <div className="task-suggestion-input">
      <input
        ref={inputRef}
        id={id}
        name={name}
        autoComplete="off"
        spellCheck={false}
        value={value}
        role="combobox"
        aria-autocomplete="list"
        aria-expanded={open}
        aria-controls={open ? listId : undefined}
        aria-activedescendant={open && items[active] !== undefined ? `${listId}-${active}` : undefined}
        aria-invalid={error === undefined ? undefined : true}
        aria-describedby={error === undefined ? undefined : errorId}
        onFocus={() => { setActive(-1); setExpanded(true) }}
        onChange={(event) => {
          onChange(event.currentTarget.value)
          setResult(null)
          setActive(-1)
          setExpanded(true)
        }}
        onKeyDown={(event) => {
          if (event.nativeEvent.isComposing) return
          if (event.key === "ArrowDown" || event.key === "ArrowUp") {
            event.preventDefault()
            setExpanded(true)
            setActive(items.length === 0 ? -1 : event.key === "ArrowDown" ? (active + 1) % items.length : (active <= 0 ? items.length : active) - 1)
          } else if (event.key === "Enter" && open && items[active] !== undefined) {
            event.preventDefault()
            choose(items[active])
          } else if (event.key === "Escape" && open) {
            event.preventDefault()
            event.stopPropagation()
            setExpanded(false)
          } else if (event.key === "Tab") {
            setExpanded(false)
          }
        }}
      />
        <button
          type="button"
          className="task-suggestion-toggle"
          aria-label={`Show ${label} suggestions`}
          title={`Show ${label} suggestions`}
          disabled={!enabled}
          tabIndex={-1}
          onMouseDown={(event) => event.preventDefault()}
          onClick={() => { inputRef.current?.focus(); setExpanded(!open); setActive(-1) }}
        ><ChevronDown size={16} aria-hidden="true" /></button>
      </div>
      {open ? <div className="task-suggestion-popup">
        <ul ref={listRef} id={listId} role="listbox" aria-label={`${label} suggestions`}>
          {items.map((item, index) => {
            const Icon = item.kind === "directory" ? Folder : item.kind === "file" ? File : Workflow
            return <li
              key={item.value}
              id={`${listId}-${index}`}
              role="option"
              aria-selected={active === index}
              onMouseDown={(event) => event.preventDefault()}
              onMouseMove={() => setActive(index)}
              onClick={() => choose(item)}
            >
              <Icon size={16} aria-hidden="true" />
              <span title={item.value}>{name === "workflow" ? item.value : item.value.replace(/[\\/]+$/, "").split(/[\\/]/).at(-1)}</span>
            </li>
          })}
        </ul>
        <div className="task-suggestion-status" role="status">
          {current === null ? "Loading suggestions..." : current.failed ? "Suggestions unavailable." : items.length === 0 ? "No matches." : current.truncated ? "More matches available. Refine the path or name." : null}
        </div>
      </div> : null}
      {error === undefined ? null : <small id={errorId}>{error}</small>}
    </div>
  )
}
