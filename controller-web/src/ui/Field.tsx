import type { InputHTMLAttributes, ReactNode, Ref } from "react"

type BaseFieldProps = {
  readonly id: string
  readonly label: string
  readonly error?: string | undefined
  readonly hint?: ReactNode
}

type InputFieldProps = BaseFieldProps &
  Omit<InputHTMLAttributes<HTMLInputElement>, "id" | "className"> & {
    readonly ref?: Ref<HTMLInputElement>
  }

/**
 * A labelled control with programmatically associated error text.
 *
 * The label is always visible: no placeholder-as-label anywhere in the product.
 */
export function Field({ id, label, error, hint, ...input }: InputFieldProps) {
  const errorId = `${id}-error`
  const hintId = `${id}-hint`
  const describedBy = [error === undefined ? null : errorId, hint === undefined ? null : hintId]
    .filter((value) => value !== null)
    .join(" ")
  return (
    <label className="field" htmlFor={id}>
      <span>{label}</span>
      <input
        {...input}
        id={id}
        aria-invalid={error === undefined ? undefined : true}
        aria-describedby={describedBy === "" ? undefined : describedBy}
      />
      {hint === undefined ? null : <small id={hintId} className="field-hint">{hint}</small>}
      {error === undefined ? null : <small id={errorId} role="alert">{error}</small>}
    </label>
  )
}

type CheckFieldProps = {
  readonly id: string
  readonly name: string
  readonly label: string
  readonly checked: boolean
  readonly onChange: (checked: boolean) => void
}

export function CheckField({ id, name, label, checked, onChange }: CheckFieldProps) {
  return (
    <label className="field field--check" htmlFor={id}>
      <input id={id} name={name} type="checkbox" checked={checked} onChange={(event) => onChange(event.currentTarget.checked)} />
      <span>{label}</span>
    </label>
  )
}
