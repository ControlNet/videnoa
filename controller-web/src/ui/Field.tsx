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

type SegmentedOption<Value extends string> = {
  readonly value: Value
  readonly label: string
}

type SegmentedFieldProps<Value extends string> = {
  readonly name: string
  readonly label: string
  readonly value: Value
  readonly options: readonly SegmentedOption<Value>[]
  readonly onChange: (value: Value) => void
}

/**
 * One choice among a closed set of peers, with every option visible.
 *
 * A set this small does not belong behind a dropdown: collapsing it hides most
 * of the decision and charges a click to see the rest. It matters more when the
 * choice changes what a later field means, because then the selection is the
 * explanation for what the form is asking next.
 *
 * Radios rather than buttons. The group is a single tab stop, arrow keys move
 * within it, the legend names it, and assistive technology announces position
 * in the set -- all of that is the platform's, and none of it survives being
 * rebuilt out of buttons and `aria-pressed`.
 */
export function SegmentedField<Value extends string>({ name, label, value, options, onChange }: SegmentedFieldProps<Value>) {
  return (
    <fieldset className="field field--segmented">
      <legend>{label}</legend>
      <div className="segmented">
        {options.map((option) => (
          <label key={option.value} className={option.value === value ? "segment segment--selected" : "segment"}>
            <input
              type="radio"
              name={name}
              value={option.value}
              checked={option.value === value}
              onChange={() => onChange(option.value)}
            />
            <span>{option.label}</span>
          </label>
        ))}
      </div>
    </fieldset>
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
