import { Loader2, Play } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { WorkflowEntry } from "@/api/client";
import { listPresets, listWorkflows, submitJob } from "@/api/client";
import { PortField } from "@/components/shared/PortField";
import type { ParamValue } from "@/components/shared/port-field-utils";
import {
	buildDefaults,
	convertParam,
	getDefaultValue,
} from "@/components/shared/port-field-utils";
import { toast } from "@/components/shared/Toaster";
import { presetToEntry } from "@/components/shared/workflow-list-utils";
import { Button } from "@/components/ui/button";
import {
	Card,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogHeader,
	DialogTitle,
} from "@/components/ui/dialog";
import { formatErrorWithPrefix } from "@/lib/presentation-error";

interface RunWorkflowDialogProps {
	open: boolean;
	onOpenChange: (open: boolean) => void;
	onSubmitted: () => void;
}

// ─── Component ──────────────────────────────────────────────────────────────

export function RunWorkflowDialog({
	open,
	onOpenChange,
	onSubmitted,
}: RunWorkflowDialogProps) {
	const { t } = useTranslation("jobs");
	const [workflows, setWorkflows] = useState<WorkflowEntry[]>([]);
	const [selectedWorkflow, setSelectedWorkflow] =
		useState<WorkflowEntry | null>(null);
	const [paramValues, setParamValues] = useState<Record<string, ParamValue>>(
		{},
	);
	const [loading, setLoading] = useState(false);
	const [submitting, setSubmitting] = useState(false);

	// Fetch workflows when dialog opens
	useEffect(() => {
		if (!open) return;
		setLoading(true);
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
			.catch((err: unknown) => {
				console.error("Failed to fetch workflows:", err);
				toast.error(t("jobs.dialog.errors.loadWorkflows"));
			})
			.finally(() => {
				setLoading(false);
			});
	}, [open, t]);

	// Reset state when dialog closes
	useEffect(() => {
		if (!open) {
			setSelectedWorkflow(null);
			setParamValues({});
		}
	}, [open]);

	const handleSelect = useCallback((entry: WorkflowEntry) => {
		setSelectedWorkflow(entry);
		const inputs = entry.workflow.interface?.inputs ?? [];
		setParamValues(buildDefaults(inputs));
	}, []);

	const handleParamChange = useCallback((name: string, value: ParamValue) => {
		setParamValues((prev) => ({ ...prev, [name]: value }));
	}, []);

	const handleSubmit = useCallback(async () => {
		if (!selectedWorkflow) return;
		setSubmitting(true);
		try {
			const inputs = selectedWorkflow.workflow.interface?.inputs ?? [];
			const modifiedWorkflow = {
				...selectedWorkflow.workflow,
				nodes: selectedWorkflow.workflow.nodes.map((node) => {
					if (node.node_type === "WorkflowInput") {
						return {
							...node,
							params: {
								...node.params,
								...Object.fromEntries(
									inputs.map((port) => [
										port.name,
										convertParam(
											port,
											paramValues[port.name] ?? getDefaultValue(port),
										),
									]),
								),
							},
						};
					}
					return node;
				}),
			};
			await submitJob(modifiedWorkflow, {
				workflowName: selectedWorkflow.name,
			});
			toast.success(t("jobs.dialog.success.submitted"));
			onOpenChange(false);
			onSubmitted();
		} catch (err: unknown) {
			toast.error(
				formatErrorWithPrefix(t("jobs.dialog.errors.submitPrefix"), err),
			);
		} finally {
			setSubmitting(false);
		}
	}, [selectedWorkflow, paramValues, onOpenChange, onSubmitted, t]);

	const inputs = selectedWorkflow?.workflow.interface?.inputs ?? [];

	return (
		<Dialog open={open} onOpenChange={onOpenChange}>
			<DialogContent className="max-w-xl">
				<DialogHeader>
					<DialogTitle>{t("jobs.dialog.title")}</DialogTitle>
					<DialogDescription>{t("jobs.dialog.description")}</DialogDescription>
				</DialogHeader>

				{/* Loading state */}
				{loading && (
					<div className="flex flex-col items-center justify-center gap-2 py-12">
						<Loader2
							className="h-5 w-5 animate-spin text-muted-foreground"
							aria-hidden="true"
						/>
						<p className="text-xs text-muted-foreground">
							{t("jobs.dialog.loading.workflows")}
						</p>
					</div>
				)}

				{/* Empty state */}
				{!loading && workflows.length === 0 && (
					<div className="flex flex-col items-center justify-center py-10 text-center">
						<div className="flex h-12 w-12 items-center justify-center rounded-xl bg-secondary/60 mb-3">
							<Play className="h-5 w-5 text-muted-foreground" />
						</div>
						<p className="text-sm text-muted-foreground max-w-[300px]">
							{t("jobs.dialog.empty.noWorkflowWithInputs")}
						</p>
					</div>
				)}

				{/* Workflow selector */}
				{!loading && workflows.length > 0 && (
					<div className="space-y-4">
						<div className="max-h-[260px] overflow-y-auto pr-1">
							<div className="grid gap-2">
								{workflows.map((entry) => {
									const isSelected =
										selectedWorkflow?.filename === entry.filename;
									return (
										<Card
											key={entry.filename}
											className={`cursor-pointer transition-colors ${
												isSelected
													? "border-primary bg-primary/5"
													: "hover:bg-secondary/40"
											}`}
											onClick={() => {
												handleSelect(entry);
											}}
										>
											<CardHeader className="p-3">
												<CardTitle className="text-sm">{entry.name}</CardTitle>
												{entry.description && (
													<CardDescription className="text-xs">
														{entry.description}
													</CardDescription>
												)}
											</CardHeader>
										</Card>
									);
								})}
							</div>
						</div>

						{/* Parameter form */}
						{selectedWorkflow && inputs.length > 0 && (
							<div className="space-y-3">
								<p className="text-xs font-medium text-muted-foreground uppercase tracking-wider">
									{t("jobs.dialog.parameters.title")}
								</p>
								<div className="space-y-3">
									{inputs.map((port) => (
										<PortField
											key={port.name}
											port={port}
											value={paramValues[port.name] ?? getDefaultValue(port)}
											onChange={(v) => {
												handleParamChange(port.name, v);
											}}
										/>
									))}
								</div>
							</div>
						)}

						{/* Submit */}
						{selectedWorkflow && (
							<Button
								className="w-full"
								onClick={() => {
									void handleSubmit();
								}}
								disabled={submitting}
							>
								{submitting ? (
									<Loader2 className="h-3.5 w-3.5 animate-spin" />
								) : (
									<Play className="h-3.5 w-3.5" />
								)}
								{submitting
									? t("jobs.dialog.actions.submitting")
									: t("jobs.dialog.actions.run")}
							</Button>
						)}
					</div>
				)}
			</DialogContent>
		</Dialog>
	);
}
