import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

/*
 * Settings density, borrowed from the Controller: a section is a 13px heading,
 * a hairline, and a grid of 32px fields with 11px labels and monospace values.
 * A runtime number does not need a 340px input, so the grid packs several
 * fields per row instead of two.
 */

export function SettingsSection({
	id,
	title,
	note,
	children,
}: {
	id: string;
	title: string;
	note?: ReactNode;
	children: ReactNode;
}) {
	return (
		<section
			id={id}
			aria-label={title}
			className="flex min-w-0 scroll-mt-16 flex-col gap-3"
		>
			<header className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1 border-b border-border pb-2">
				<h3 className="text-[13px] font-semibold leading-[18px]">{title}</h3>
				{note === undefined ? null : (
					<p className="text-[11px] leading-4 text-muted-foreground/90">{note}</p>
				)}
			</header>
			{children}
		</section>
	);
}

export function SettingsGrid({
	children,
	className,
}: {
	children: ReactNode;
	className?: string;
}) {
	return (
		<div
			className={cn(
				"grid grid-cols-1 items-start gap-x-4 gap-y-3 sm:grid-cols-6",
				className,
			)}
		>
			{children}
		</div>
	);
}

/** A visible label, the config key beside it, and the control below. */
export function Field({
	id,
	label,
	hint,
	className,
	children,
}: {
	id: string;
	label: string;
	hint?: string;
	className?: string;
	children: ReactNode;
}) {
	return (
		<div className={cn("flex min-w-0 flex-col gap-1", className)}>
			<label
				htmlFor={id}
				className="flex items-baseline justify-between gap-2 text-[11px] font-semibold leading-4 text-muted-foreground"
			>
				<span className="truncate">{label}</span>
				{hint === undefined ? null : (
					<span className="shrink-0 font-mono font-normal">{hint}</span>
				)}
			</label>
			{children}
		</div>
	);
}

/** Recessed control on a raised page, so a field reads as somewhere to type. */
export const settingsInputClass =
	"h-8 rounded-md border-border bg-background-deep px-3 font-mono text-[12.5px] shadow-none";

export const settingsCheckboxClass =
	"size-4 shrink-0 cursor-pointer rounded border-border accent-primary";

/** A checkbox that lines up with the fields beside it. */
export function CheckRow({
	id,
	label,
	className,
	children,
	...input
}: {
	id: string;
	label: ReactNode;
	className?: string;
	children?: ReactNode;
} & React.InputHTMLAttributes<HTMLInputElement>) {
	return (
		<div className={cn("flex h-8 min-w-0 items-center gap-2", className)}>
			<input
				{...input}
				id={id}
				type="checkbox"
				className={settingsCheckboxClass}
			/>
			<label htmlFor={id} className="cursor-pointer text-[12.5px]">
				{label}
			</label>
			{children}
		</div>
	);
}
