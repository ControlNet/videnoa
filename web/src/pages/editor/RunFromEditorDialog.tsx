import { Loader2, Play } from "lucide-react";
import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router";
import { PortField } from "@/components/shared/PortField";
import type { ParamValue } from "@/components/shared/port-field-utils";
import {
	buildDefaults,
	convertParam,
	getDefaultValue,
} from "@/components/shared/port-field-utils";
import { toast } from "@/components/shared/Toaster";
import { Button } from "@/components/ui/button";
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogHeader,
	DialogTitle,
} from "@/components/ui/dialog";
import { formatErrorWithPrefix } from "@/lib/presentation-error";
import { useJobStore } from "@/stores/job-store";
import { useWorkflowStore } from "@/stores/workflow-store";
import type { WorkflowPort } from "@/types";

interface RunFromEditorDialogProps {
	open: boolean;
	onOpenChange: (open: boolean) => void;
	inputs: WorkflowPort[];
}

export function RunFromEditorDialog({
	open,
	onOpenChange,
	inputs,
}: RunFromEditorDialogProps) {
	const { t } = useTranslation("editor");
	const navigate = useNavigate();
	const [paramValues, setParamValues] = useState<Record<string, ParamValue>>(
		{},
	);
	const [submitting, setSubmitting] = useState(false);

	const handleOpenChange = useCallback(
		(nextOpen: boolean) => {
			if (nextOpen) {
				setParamValues(buildDefaults(inputs));
			}
			onOpenChange(nextOpen);
		},
		[inputs, onOpenChange],
	);

	const handleParamChange = useCallback((name: string, value: ParamValue) => {
		setParamValues((prev) => ({ ...prev, [name]: value }));
	}, []);

	const handleSubmit = useCallback(async () => {
		setSubmitting(true);
		try {
			const workflowState = useWorkflowStore.getState();
			const workflow = workflowState.exportWorkflow();
			const workflowName = workflowState.currentFile?.name ?? "Editor workflow";
			const modifiedWorkflow = {
				...workflow,
				nodes: workflow.nodes.map((node) => {
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
			await useJobStore.getState().submitJob(modifiedWorkflow, {
				workflowName,
			});
			toast.success(t("runDialog.success.submitted"));
			onOpenChange(false);
			void navigate("/jobs");
		} catch (err: unknown) {
			toast.error(
				formatErrorWithPrefix(t("runDialog.errors.submitPrefix"), err),
			);
		} finally {
			setSubmitting(false);
		}
	}, [inputs, navigate, onOpenChange, paramValues, t]);

	return (
		<Dialog open={open} onOpenChange={handleOpenChange}>
			<DialogContent className="max-w-md">
				<DialogHeader>
					<DialogTitle>{t("runDialog.title")}</DialogTitle>
					<DialogDescription>{t("runDialog.description")}</DialogDescription>
				</DialogHeader>

				<div className="space-y-3">
					<p className="text-xs font-medium text-muted-foreground uppercase tracking-wider">
						{t("runDialog.parameters.title")}
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
						? t("runDialog.actions.submitting")
						: t("runDialog.actions.run")}
				</Button>
			</DialogContent>
		</Dialog>
	);
}
