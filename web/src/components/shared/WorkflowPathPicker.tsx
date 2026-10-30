import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { WorkflowEntry } from "@/api/client";
import { listPresets, listWorkflows } from "@/api/client";
import { Button } from "@/components/ui/button";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "@/components/ui/select";
import { presetToEntry } from "./workflow-list-utils";

interface WorkflowPathPickerProps {
	value: string;
	onChange: (value: string) => void;
}

export function WorkflowPathPicker({
	value,
	onChange,
}: WorkflowPathPickerProps) {
	const { t } = useTranslation("common");
	const [workflows, setWorkflows] = useState<WorkflowEntry[]>([]);
	const [loading, setLoading] = useState(true);
	/** A list failed to load; that must not read as "no workflows". */
	const [failed, setFailed] = useState(false);
	const [reloadKey, setReloadKey] = useState(0);

	useEffect(() => {
		let cancelled = false;
		void (async () => {
			const [p, w] = await Promise.allSettled([listPresets(), listWorkflows()]);
			if (cancelled) return;
			const presets = p.status === "fulfilled" ? p.value : [];
			const saved = w.status === "fulfilled" ? w.value : [];
			const fromPresets = presets
				.map(presetToEntry)
				.filter((e) => e.has_interface);
			const fromSaved = saved.filter((e) => e.has_interface);
			const seen = new Set(fromSaved.map((e) => e.filename));
			const merged = [
				...fromSaved,
				...fromPresets.filter((e) => !seen.has(e.filename)),
			];
			merged.sort((a, b) =>
				a.name.toLowerCase().localeCompare(b.name.toLowerCase()),
			);
			setWorkflows(merged);
			setFailed(p.status === "rejected" || w.status === "rejected");
			setLoading(false);
		})();
		return () => {
			cancelled = true;
		};
	}, [reloadKey]);

	const retry = useCallback(() => {
		setLoading(true);
		setReloadKey((k) => k + 1);
	}, []);

	return (
		<div className="flex flex-col gap-1">
			<Select value={value} onValueChange={onChange}>
				<SelectTrigger
					className="h-7 text-xs"
					aria-label={t("workflowPathPicker.triggerAriaLabel")}
				>
					<SelectValue
						placeholder={
							loading
								? t("workflowPathPicker.loadingPlaceholder")
								: t("workflowPathPicker.selectPlaceholder")
						}
					/>
				</SelectTrigger>
				<SelectContent>
					{workflows.map((wf) => (
						<SelectItem key={wf.filename} value={wf.filename}>
							{wf.name}
						</SelectItem>
					))}
					{!loading && !failed && workflows.length === 0 && (
						<div className="px-2 py-1.5 text-xs text-muted-foreground">
							{t("workflowPathPicker.empty")}
						</div>
					)}
				</SelectContent>
			</Select>
			{!loading && failed && (
				<div
					role="alert"
					className="flex items-center gap-1.5 text-[11px] text-destructive"
				>
					<span>{t("workflowPathPicker.error")}</span>
					<Button
						variant="outline"
						size="sm"
						className="h-5 px-1.5 text-[11px]"
						onClick={retry}
					>
						{t("workflowPathPicker.retry")}
					</Button>
				</div>
			)}
		</div>
	);
}
