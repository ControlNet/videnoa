import { ArrowLeft, Eye, Layers, RotateCcw, Trash2, X } from "lucide-react"
import { useEffect, useRef, useState } from "react"

import type { ApiClient } from "../api/client"
import { taskCreateResponseSchema } from "../api/taskSchemas"
import { Button } from "../ui/Button"
import { ManualTaskField } from "./ManualTaskField"
import { batchError, batchPreviewSchema, batchRowErrors, initialBatchOptions, type BatchOptions, type BatchRow } from "./batchTask"
import { beginSubmission } from "./submissionIntent"
import "./batch-task.css"

type Props = { apiClient: ApiClient; onClose: () => void; onCreated: () => void }

export function BatchTaskDialog({ apiClient, onClose, onCreated }: Props) {
  const dialogRef = useRef<HTMLDialogElement>(null)
  const titleRef = useRef<HTMLHeadingElement>(null)
  const inputRef = useRef<HTMLInputElement>(null)
  const outputRef = useRef<HTMLInputElement>(null)
  const workflowRef = useRef<HTMLInputElement>(null)
  const [options, setOptions] = useState<BatchOptions>(initialBatchOptions)
  const [rows, setRows] = useState<BatchRow[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [started, setStarted] = useState(false)
  const busyRef = useRef(false)
  const reviewing = rows !== null
  const created = rows?.filter((row) => row.status === "created").length ?? 0
  const selected = rows?.filter((row) => !row.excluded) ?? []
  const rowErrors = batchRowErrors(rows ?? [])
  const conflicts = selected.filter((row) => rowErrors.get(row.request.input_path) !== null).length
  const complete = selected.length > 0 && created === selected.length
  const canCreate = selected.length > 0 && conflicts === 0 && !complete

  useEffect(() => {
    dialogRef.current?.showModal()
    inputRef.current?.focus()
  }, [])

  useEffect(() => {
    if (reviewing) titleRef.current?.focus()
    else inputRef.current?.focus()
  }, [reviewing])

  function back() {
    if (busyRef.current || started) return
    setRows(null)
    setError(null)
  }

  function toggleRow(inputPath: string) {
    if (busyRef.current || started) return
    setRows((current) => current?.map((row) => row.request.input_path === inputPath ? { ...row, excluded: !row.excluded } : row) ?? null)
  }

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
      setRows(preview.items.map((row) => ({ ...row, excluded: false, intent: beginSubmission(null, row.request), status: "ready", submissionError: null })))
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
        if (row.excluded || row.status === "created") continue
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
      <header><div><p className="technical-label">{reviewing ? "STEP 2 OF 2" : "STEP 1 OF 2"}</p><h2 ref={titleRef} tabIndex={-1} id="batch-task-title">{reviewing ? "Preview Tasks" : "Add Batch"}</h2></div>
        <Button variant="outline" size="sm" icon aria-label={reviewing ? "Close Preview Tasks" : "Close Add Batch"} disabled={busy} onClick={close}><X size={16} aria-hidden="true" /></Button>
      </header>
      {error === null ? null : <div className="task-action-error alert alert--danger" role="alert">{error}</div>}
      {reviewing ? <dl className="batch-review-summary">
        <div><dt>Input Pattern</dt><dd>{options.input_pattern}</dd></div>
        <div><dt>Workflow</dt><dd>{options.workflow}</dd></div>
        <div><dt>Priority</dt><dd>{options.priority}</dd></div>
      </dl> : <fieldset className="batch-task-fields" disabled={busy || started}>
        <ManualTaskField apiClient={apiClient} enabled={!busy && !started} idPrefix="batch" label="Input Pattern" name="input_pattern" value={options.input_pattern} error={undefined} inputRef={inputRef} onChange={(input_pattern) => update({ input_pattern })} />
        <p className="batch-help">{"Use * for filenames, ? for one character, ** for nested directories, or *.{mkv,mp4} for multiple formats. Up to 500 files."}</p>
        <label className="field"><span>Output Mode</span><select value={options.output_mode} onChange={(event) => update({ output_mode: event.target.value as BatchOptions["output_mode"], naming_mode: event.target.value === "beside_input" && options.naming_mode === "original" ? "insert_extension" : options.naming_mode })}>
          <option value="beside_input">Beside each input file</option><option value="directory">One output directory</option>
        </select></label>
        {options.output_mode === "directory" ? <ManualTaskField apiClient={apiClient} enabled={!busy && !started} idPrefix="batch" label="Output Directory" name="output_path" value={options.output_directory} error={undefined} inputRef={outputRef} onChange={(output_directory) => update({ output_directory })} /> : null}
        <div className="batch-field-pair">
          <label className="field"><span>Filename Format</span><select value={options.naming_mode} onChange={(event) => update({ naming_mode: event.target.value as BatchOptions["naming_mode"] })}>
            <option value="insert_extension">stem.middle.extension</option><option value="jellyfin_version_suffix">Jellyfin version suffix</option><option value="original" disabled={options.output_mode !== "directory"}>Original filename</option>
          </select></label>
          {options.naming_mode !== "original" ? <label className="field"><span>{options.naming_mode === "jellyfin_version_suffix" ? "Version Label" : "Middle Extension"}</span><input aria-describedby={options.naming_mode === "jellyfin_version_suffix" ? "batch-version-help" : undefined} value={options.middle_extension} maxLength={64} autoComplete="off" spellCheck={false} onChange={(event) => update({ middle_extension: event.target.value })} /></label> : null}
        </div>
        {options.naming_mode === "jellyfin_version_suffix" ? <p className="batch-help" id="batch-version-help">
          Example: Re Zero S03E01 - {options.middle_extension}.mkv<br />
          Appends to the existing filename stem. Episode auto-grouping requires Jellyfin 12 with both versions in the same season folder of a TV library.
        </p> : null}
        <div className="batch-field-pair">
          <ManualTaskField apiClient={apiClient} enabled={!busy && !started} idPrefix="batch" label="Workflow" name="workflow" value={options.workflow} error={undefined} inputRef={workflowRef} onChange={(workflow) => update({ workflow })} />
          <label className="field"><span>Priority</span><input type="number" min={-100} max={100} step={1} value={options.priority} onChange={(event) => update({ priority: event.target.value })} /></label>
        </div>
      </fieldset>}
      {rows === null ? null : <section className="batch-preview" aria-label="Batch preview">
        {!started && conflicts > 0 ? <p>Remove conflicting tasks below or go back to change the batch settings.</p> : null}
        <p role="status">{started ? `${created} of ${selected.length} tasks created` : `${rows.length} matching files · ${selected.length} selected · ${rows.length - selected.length} removed · ${conflicts} conflicts`}</p>
        {rows.length > 0 && selected.length === 0 ? <p>All tasks are removed. Restore at least one task to create this batch.</p> : null}
        {rows.length === 0 ? <p>No files matched. Adjust the input pattern and preview again.</p> : <div className="batch-preview-scroll" tabIndex={0} role="region" aria-label="Preview task paths">
          <table><thead><tr><th scope="col">Input Path</th><th scope="col">Output Path</th><th scope="col">Status</th><th scope="col" className="batch-row-actions">Actions</th></tr></thead>
            <tbody>{rows.map((row) => <tr key={row.request.input_path} className={row.excluded ? "batch-row--excluded" : undefined}>
              <td className="batch-row-path">{row.request.input_path}</td><td className="batch-row-path">{row.request.output_path}</td><td>{row.excluded ? "Removed" : rowErrors.get(row.request.input_path) ?? row.submissionError ?? ({ ready: "Ready", creating: "Creating…", created: "Created", failed: "Failed" }[row.status])}</td>
              <td className="batch-row-actions"><Button variant="outline" size="sm" icon disabled={busy || started}
                aria-label={`${row.excluded ? "Restore" : "Remove"} task ${row.request.input_path}`}
                title={row.excluded ? "Restore task" : "Remove task"}
                onClick={() => toggleRow(row.request.input_path)}>
                {row.excluded ? <RotateCcw size={15} aria-hidden="true" /> : <Trash2 size={15} aria-hidden="true" />}
              </Button></td>
            </tr>)}</tbody>
          </table>
        </div>}
      </section>}
      {started && !complete ? <p>Keep this dialog open to retry safely. The previewed task list stays fixed once creation starts.</p> : null}
      <footer>
        {reviewing && !complete ? <Button variant="outline" disabled={busy || started} onClick={back}><ArrowLeft size={16} aria-hidden="true" />Back</Button> : null}
        <span className="spacer" />
        {!reviewing ? <Button variant="primary" type="submit" disabled={busy}><Eye size={16} aria-hidden="true" />{busy ? "Previewing…" : "Preview Tasks"}</Button>
          : complete ? <Button variant="primary" onClick={close}>Done</Button>
            : <Button variant="primary" type="submit" disabled={busy || !canCreate}><Layers size={16} aria-hidden="true" />{busy ? `Creating ${created}/${selected.length}…` : started ? "Retry Remaining" : "Create Tasks"}</Button>}
      </footer>
    </form>
  </dialog>
}
