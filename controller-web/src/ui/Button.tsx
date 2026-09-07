import type { ButtonHTMLAttributes, Ref } from "react"

export type ButtonVariant = "primary" | "outline" | "ghost" | "danger"
export type ButtonSize = "md" | "sm"

type ButtonProps = Omit<ButtonHTMLAttributes<HTMLButtonElement>, "className"> & {
  readonly variant?: ButtonVariant
  readonly size?: ButtonSize
  /** Square icon-only button; supply an aria-label. */
  readonly icon?: boolean
  readonly ref?: Ref<HTMLButtonElement>
}

export function Button({ variant = "ghost", size = "md", icon = false, type = "button", ...rest }: ButtonProps) {
  const classes = ["button", `button--${variant}`]
  if (size !== "md") classes.push(`button--${size}`)
  if (icon) classes.push("button--icon")
  // biome-ignore lint/a11y/useButtonType: the type is resolved from props above.
  return <button {...rest} type={type} className={classes.join(" ")} />
}
