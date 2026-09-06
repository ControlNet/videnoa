import { Eye, Layers, X } from "lucide-react"
import { useEffect, useRef, useState } from "react"

import type { ApiClient } from "../api/client"
import { taskCreateResponseSchema } from "../api/taskSchemas"
import { Button } from "../ui/Button"
import { ManualTaskField } from "./ManualTaskField"
import { batchError, batchPreviewSchema, initialBatchOptions, type BatchOptions, type BatchRow } from "./batchTask"
import { beginSubmission } from "./submissionIntent"
import "./batch-task.css"

type Props = { apiClient: ApiClient; onClose: () => void; onCreated: () => void }

export function BatchTaskDialog({ apiClient, onClose, onCreated }: Props) {
  const dialogRef = useRef<HTMLDialogElement>(null)
  const inputRef = useRef<HTMLInputElement>(null)
  const outputRef = useRef<HTMLInputElement>(null)
  const workflowRef = useRef<HTMLInputElement>(null)
  const [options, setOptions] = useState<BatchOptions>(initialBatchOptions)
  const [rows, setRows] = useState<BatchRow[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [started, setStarted] = useState(false)
  const busyRef = useRef(false)
  const created = rows?.filter((row) => row.status === "created").length ?? 0
  const complete = rows !== null && rows.length > 0 && created === rows.length
  const canCreate = rows !== null && rows.length > 0 && rows.every((row) => row.error === null) && !complete

  useEffect(() => {
    dialogRef.current?.showModal()
    inputRef.current?.focus()
  }, [])

  function update(patch: Partial<BatchOptions>) {
    setOptions({ ...options, ...patch })
    setRows(null)
    setError(null)
  }

  async function preview() {
    if (busyRef.current || started) return
    if (options.priority === "" || !Number.isInteger(Number(options.priority))) {
      setError("Priority must be a whole number between -100 and 100.")
      return
    }
    busyRef.current = true
    setBusy(true)
    setError(null)
    setRows(null)
    try {
      const preview = await apiClient.request("api/tasks/batch-preview", {
        method: "POST", json: { ...options, priority: Number(options.priority) }, schema: batchPreviewSchema,
      })
      setRows(preview.items.map((row) => ({ ...row, intent: beginSubmission(null, row.request), status: "ready", submissionError: null })))
    } catch (cause) {
      setError(batchError(cause))
    } finally {
      busyRef.current = false
      setBusy(false)
    }
  }

  async function create() {
    if (busyRef.current || !canCreate || rows === null) return
    busyRef.current = true
    setBusy(true)
    setStarted(true)
    setError(null)
    const next = [...rows]
    try {
      for (let index = 0; index < next.length; index += 1) {
        const row = next[index]
        if (row === undefined) continue
        if (row.status === "created") continue
        next[index] = { ...row, status: "creating", submissionError: null }
        setRows([...next])
        try {
          await apiClient.request("api/tasks", {
            method: "POST", headers: { "Idempotency-Key": row.intent.key },
            json: row.intent.body, schema: taskCreateResponseSchema,
          })
          next[index] = { ...row, status: "created", submissionError: null }
        } catch (cause) {
          next[index] = { ...row, status: "failed", submissionError: batchError(cause) }
          setRows([...next])
          setError("Creation paused. Completed tasks are kept; retry continues with the remaining tasks.")
          break
        }
        setRows([...next])
      }
    } finally {
      busyRef.current = false
      setBusy(false)
      onCreated()
    }
  }

  function close() {
    if (!busyRef.current) onClose()
  }

  return <dialog ref={dialogRef} className="dialog task-dialog batch-task-dialog" aria-labelledby="batch-task-title" onCancel={(event) => { event.preventDefault(); close() }}>
    <form className="task-create-form" onSubmit={(event) => { event.preventDefault(); void (rows === null ? preview() : create()) }}>
      <header><div><p className="technical-label">BATCH INTAKE</p><h2 id="batch-task-title">Add Batch</h2></div>
        <Button variant="outline" size="sm" icon aria-label="Close Add Batch" disabled={busy} onClick={close}><X size={16} aria-hidden="true" /></Button>
      </header>
      <p>Match files, preview their output paths, then create tasks with one workflow and priority.</p>
      {error === null ? null : <div className="task-action-error alert alert--danger" role="alert">{error}</div>}
      <fieldset className="batch-task-fields" disabled={busy || started}>
        <ManualTaskField apiClient={apiClient} enabled={!busy && !started} idPrefix="batch" label="Input Pattern" name="input_pattern" value={options.input_pattern} error={undefined} inputRef={inputRef} onChange={(input_pattern) => update({ input_pattern })} />
        <p className="batch-help">Use * for filenames, ? for one character, or ** for nested directories. Up to 500 files.</p>
        <label className="field"><span>Output Mode</span><select value={options.output_mode} onChange={(event) => update({ output_mode: event.target.value as BatchOptions["output_mode"], naming_mode: "insert_extension" })}>
          <option value="beside_input">Beside each input file</option><option value="directory">One output directory</option>
        </select></label>
        {options.output_mode === "directory" ? <ManualTaskField apiClient={apiClient} enabled={!busy && !started} idPrefix="batch" label="Output Directory" name="output_path" value={options.output_directory} error={undefined} inputRef={outputRef} onChange={(output_directory) => update({ output_directory })} /> : null}
        <div className="batch-field-pair">
          <label className="field"><span>Filename Format</span><select value={options.naming_mode} onChange={(event) => update({ naming_mode: event.target.value as BatchOptions["naming_mode"] })}>
            <option value="insert_extension">stem.middle.extension</option><option value="original" disabled={options.output_mode !== "directory"}>Original filename</option>
          </select></label>
          {options.naming_mode === "insert_extension" ? <label className="field"><span>Middle Extension</span><input value={options.middle_extension} maxLength={64} autoComplete="off" spellCheck={false} onChange={(event) => update({ middle_extension: event.target.value })} /></label> : null}
        </div>
        <div className="batch-field-pair">
          <ManualTaskField apiClient={apiClient} enabled={!busy && !started} idPrefix="batch" label="Workflow" name="workflow" value={options.workflow} error={undefined} inputRef={workflowRef} onChange={(workflow) => update({ workflow })} />
          <label className="field"><span>Priority</span><input type="number" min={-100} max={100} step={1} value={options.priority} onChange={(event) => update({ priority: event.target.value })} /></label>
        </div>
      </fieldset>
      {rows === null ? null : <section className="batch-preview" aria-label="Batch preview">
        <p role="status">{started ? `${created} of ${rows.length} tasks created` : `${rows.length} matching files · ${rows.filter((row) => row.error !== null).length} conflicts`}</p>
        {rows.length === 0 ? <p>No files matched. Adjust the input pattern and preview again.</p> : <div className="batch-preview-scroll" tabIndex={0} role="region" aria-label="Preview task paths">
          <table><thead><tr><th scope="col">Input Path</th><th scope="col">Output Path</th><th scope="col">Status</th></tr></thead>
            <tbody>{rows.map((row) => <tr key={row.request.input_path}>
              <td>{row.request.input_path}</td><td>{row.request.output_path}</td><td>{row.error ?? row.submissionError ?? ({ ready: "Ready", creating: "Creating…", created: "Created", failed: "Failed" }[row.status])}</td>
            </tr>)}</tbody>
          </table>
        </div>}
      </section>}
      {started && !complete ? <p>Keep this dialog open to retry safely. Settings are locked to the previewed task list.</p> : null}
      <footer>
        <Button variant="outline" disabled={busy || started} onClick={() => { void preview() }}><Eye size={16} aria-hidden="true" />{busy && !started ? "Previewing…" : "Preview"}</Button>
        <span className="spacer" />
        {complete ? <Button variant="primary" onClick={close}>Done</Button> : <Button variant="primary" disabled={busy || !canCreate} onClick={() => { void create() }}><Layers size={16} aria-hidden="true" />{busy && started ? `Creating ${created}/${rows?.length ?? 0}…` : started ? "Retry Remaining" : "Create Tasks"}</Button>}
      </footer>
    </form>
  </dialog>
}
