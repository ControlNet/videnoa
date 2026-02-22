import type { Edge, Node } from "@xyflow/react";
import { Loader2, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { WorkflowEntry } from "@/api/client";
import { deleteWorkflow, listPresets, listWorkflows } from "@/api/client";
import { toast } from "@/components/shared/Toaster";
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
import { ScrollArea } from "@/components/ui/scroll-area";
import { useUIStore } from "@/stores/ui-store";
import { normalizeParams, useWorkflowStore } from "@/stores/workflow-store";
import type { PipelineNodeData, Preset } from "@/types";
import { computeLayout } from "./auto-layout";

export function PresetGallery() {
	const { t } = useTranslation("editor");
	const activeModal = useUIStore((s) => s.activeModal);
	const closeModal = useUIStore((s) => s.closeModal);
	const loadWorkflow = useWorkflowStore((s) => s.loadWorkflow);

	const open = activeModal === "presets";

	const [presets, setPresets] = useState<Preset[]>([]);
	const [workflows, setWorkflows] = useState<WorkflowEntry[]>([]);
	const [loading, setLoading] = useState(false);

	useEffect(() => {
		if (!open) return;
		let cancelled = false;
		setLoading(true);
		void (async () => {
			try {
				const [p, w] = await Promise.all([listPresets(), listWorkflows()]);
				if (!cancelled) {
					setPresets(p);
					setWorkflows(w);
				}
			} catch {
				if (!cancelled) {
					setPresets([]);
					setWorkflows([]);
				}
			} finally {
				if (!cancelled) {
					setLoading(false);
				}
			}
		})();
		return () => {
			cancelled = true;
		};
	}, [open]);

	const handleLoadPreset = useCallback(
		(preset: Preset) => {
			const tempNodes: Node<PipelineNodeData>[] = preset.workflow.nodes.map(
				(wn, i) => ({
					id: wn.id,
					type: "pipeline",
					position: { x: 250 * i, y: 100 },
					data: { nodeType: wn.node_type, params: normalizeParams(wn.params) },
				}),
			);
			const tempEdges: Edge[] = preset.workflow.connections.map((conn, i) => ({
				id: `e-${conn.from_node}-${conn.from_port}-${conn.to_node}-${conn.to_port}-${String(i)}`,
				source: conn.from_node,
				sourceHandle: conn.from_port,
				target: conn.to_node,
				targetHandle: conn.to_port,
				data: { port_type: conn.port_type },
			}));

			const positions = computeLayout(tempNodes, tempEdges);
			loadWorkflow(preset.workflow, positions);
			toast.success(t("gallery.toast.presetLoaded"));
			closeModal();
		},
		[closeModal, loadWorkflow, t],
	);

	const handleLoadWorkflow = useCallback(
		(entry: WorkflowEntry) => {
			const tempNodes: Node<PipelineNodeData>[] = entry.workflow.nodes.map(
				(wn, i) => ({
					id: wn.id,
					type: "pipeline",
					position: { x: 250 * i, y: 100 },
					data: { nodeType: wn.node_type, params: normalizeParams(wn.params) },
				}),
			);
			const tempEdges: Edge[] = entry.workflow.connections.map((conn, i) => ({
				id: `e-${conn.from_node}-${conn.from_port}-${conn.to_node}-${conn.to_port}-${String(i)}`,
				source: conn.from_node,
				sourceHandle: conn.from_port,
				target: conn.to_node,
				targetHandle: conn.to_port,
				data: { port_type: conn.port_type },
			}));

			const positions = computeLayout(tempNodes, tempEdges);
			loadWorkflow(entry.workflow, positions);
			useWorkflowStore.getState().setCurrentFile({
				filename: entry.filename,
				name: entry.name,
				description: entry.description,
			});
			toast.success(t("gallery.toast.workflowLoaded"));
			closeModal();
		},
		[closeModal, loadWorkflow, t],
	);

	const handleDelete = useCallback(
		async (filename: string, e: React.MouseEvent) => {
			e.stopPropagation();
			try {
				await deleteWorkflow(filename);
				setWorkflows((prev) => prev.filter((w) => w.filename !== filename));
				toast.success(t("gallery.toast.workflowDeleted"));
			} catch {
				toast.error(t("gallery.toast.deleteFailed"));
			}
		},
		[t],
	);

	return (
		<Dialog
			open={open}
			onOpenChange={(o) => {
				if (!o) closeModal();
			}}
		>
			<DialogContent className="max-w-3xl">
				<DialogHeader>
					<DialogTitle>{t("gallery.title")}</DialogTitle>
					<DialogDescription>{t("gallery.description")}</DialogDescription>
				</DialogHeader>

				{loading ? (
					<div className="flex items-center justify-center py-8">
						<Loader2 className="size-5 animate-spin text-muted-foreground" />
					</div>
				) : (
					<ScrollArea className="max-h-[500px]">
						<div className="grid gap-2">
							<p className="text-xs font-medium text-muted-foreground uppercase tracking-wide px-1">
								{t("gallery.sections.builtInPresets")}
							</p>
							{presets.map((preset) => (
								<Card
									key={preset.id}
									className="cursor-pointer hover:bg-secondary/40 transition-colors"
									onClick={() => {
										handleLoadPreset(preset);
									}}
								>
									<CardHeader className="p-4">
										<CardTitle className="text-sm">{preset.name}</CardTitle>
										{preset.description && (
											<CardDescription className="text-xs">
												{preset.description}
											</CardDescription>
										)}
									</CardHeader>
								</Card>
							))}
							{presets.length === 0 && (
								<p className="text-sm text-muted-foreground text-center py-4">
									{t("gallery.empty.noPresets")}
								</p>
							)}

							<p className="text-xs font-medium text-muted-foreground uppercase tracking-wide px-1 mt-3">
								{t("gallery.sections.savedWorkflows")}
							</p>
							{workflows.map((wf) => (
								<Card
									key={wf.filename}
									className="cursor-pointer hover:bg-secondary/40 transition-colors"
									onClick={() => {
										handleLoadWorkflow(wf);
									}}
								>
									<CardHeader className="p-4 flex flex-row items-start justify-between gap-2">
										<div className="min-w-0">
											<CardTitle className="text-sm">{wf.name}</CardTitle>
											{wf.description && (
												<CardDescription className="text-xs">
													{wf.description}
												</CardDescription>
											)}
										</div>
										<Button
											variant="ghost"
											size="icon"
											className="size-7 shrink-0 text-muted-foreground hover:text-destructive"
											onClick={(e) => {
												void handleDelete(wf.filename, e);
											}}
										>
											<Trash2 className="size-3.5" />
										</Button>
									</CardHeader>
								</Card>
							))}
							{workflows.length === 0 && (
								<p className="text-sm text-muted-foreground text-center py-4">
									{t("gallery.empty.noSavedWorkflows")}
								</p>
							)}
						</div>
					</ScrollArea>
				)}
			</DialogContent>
		</Dialog>
	);
}
