import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { WorkflowEntry } from "@/api/client";
import { listPresets, listWorkflows } from "@/api/client";
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

	useEffect(() => {
		Promise.all([listPresets(), listWorkflows()])
			.then(([presets, saved]) => {
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
			})
			.catch(() => {
				setWorkflows([]);
			})
			.finally(() => {
				setLoading(false);
			});
	}, []);

	return (
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
				{!loading && workflows.length === 0 && (
					<div className="px-2 py-1.5 text-xs text-muted-foreground">
						{t("workflowPathPicker.empty")}
					</div>
				)}
			</SelectContent>
		</Select>
	);
}
