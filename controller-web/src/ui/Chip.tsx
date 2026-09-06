import { ChevronDown } from "lucide-react"
import type { ReactNode } from "react"

/**
 * Compact filter chips.
 *
 * The chrome is compact but the control underneath stays a native select or
 * input, so keyboard behaviour, form semantics and assistive-technology
 * reporting are the platform's rather than a re-implementation.
 */

type SelectChipProps = {
  readonly label: string
  /** Visible text when the full accessible name is too long for one control row. */
  readonly shortLabel?: string
  readonly value: string
  /** Value that means "no filter applied"; drives the engaged treatment. */
  readonly neutralValue: string
  readonly onChange: (value: string) => void
  readonly children: ReactNode
}

export function SelectChip({ label, shortLabel, value, neutralValue, onChange, children }: SelectChipProps) {
  const engaged = value !== neutralValue
  return (
    <div className={engaged ? "chip chip--engaged" : "chip"}>
      <span>{shortLabel ?? label}</span>
      <select aria-label={label} value={value} onChange={(event) => onChange(event.currentTarget.value)}>
        {children}
      </select>
      <ChevronDown size={10} strokeWidth={2.5} aria-hidden="true" />
    </div>
  )
}

type TextChipProps = {
  readonly name: string
  readonly label: string
  readonly value: string
  readonly placeholder: string
  readonly onChange: (value: string) => void
}

export function TextChip({ name, label, value, placeholder, onChange }: TextChipProps) {
  return (
    <label className={value === "" ? "chip" : "chip chip--engaged"}>
      <span>{label}</span>
      <input
        name={name}
        autoComplete="off"
        spellCheck={false}
        placeholder={placeholder}
        value={value}
        onChange={(event) => onChange(event.currentTarget.value)}
      />
    </label>
  )
}
