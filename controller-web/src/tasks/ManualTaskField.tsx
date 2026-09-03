type TaskFieldProps = {
  readonly label: string
  readonly name: "input_path" | "output_path" | "workflow"
  readonly value: string
  readonly error: string | undefined
  readonly inputRef: React.RefObject<HTMLInputElement | null>
  readonly onChange: (value: string) => void
}

export function ManualTaskField({ label, name, value, error, inputRef, onChange }: TaskFieldProps) {
  const id = `task-${name.replaceAll("_", "-")}`
  const errorId = `${id}-error`
  return (
    <label className="task-form-field" htmlFor={id}>
      <span>{label}</span>
      <input
        ref={inputRef}
        id={id}
        name={name}
        autoComplete="off"
        spellCheck={false}
        value={value}
        aria-invalid={error === undefined ? undefined : true}
        aria-describedby={error === undefined ? undefined : errorId}
        onChange={(event) => onChange(event.currentTarget.value)}
      />
      {error === undefined ? null : <small id={errorId}>{error}</small>}
    </label>
  )
}
