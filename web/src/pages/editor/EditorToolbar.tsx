import {
	Download,
	Eye,
	FolderInput,
	FolderOpen,
	LayoutDashboard,
	Maximize,
	Play,
	Redo2,
	Save,
	Trash2,
	Undo2,
} from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router";
import { saveWorkflow } from "@/api/client";
import { toast } from "@/components/shared/Toaster";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
	Popover,
	PopoverContent,
	PopoverTrigger,
} from "@/components/ui/popover";
import {
	Tooltip,
	TooltipContent,
	TooltipTrigger,
} from "@/components/ui/tooltip";
import { useJobStore } from "@/stores/job-store";
import { useUIStore } from "@/stores/ui-store";
import { useWorkflowStore } from "@/stores/workflow-store";
import type { WorkflowPort } from "@/types";
import { RunFromEditorDialog } from "./RunFromEditorDialog";

interface EditorToolbarProps {
	onAutoLayout: () => void;
	onFitView: () => void;
}

function ToolbarButton({
	icon: Icon,
	label,
	onClick,
	disabled,
}: {
	icon: React.ComponentType<{ className?: string }>;
	label: string;
	onClick: () => void;
	disabled?: boolean;
}) {
	return (
		<Tooltip>
			<TooltipTrigger asChild>
				<Button
					variant="ghost"
					size="icon"
					className="size-8"
					onClick={onClick}
					disabled={disabled}
				>
					<Icon className="size-4" />
				</Button>
			</TooltipTrigger>
			<TooltipContent side="bottom">
				<span className="text-xs">{label}</span>
			</TooltipContent>
		</Tooltip>
	);
}

export function EditorToolbar({ onAutoLayout, onFitView }: EditorToolbarProps) {
	const { t } = useTranslation("editor");
	const navigate = useNavigate();
	const undo = useWorkflowStore((s) => s.undo);
	const redo = useWorkflowStore((s) => s.redo);
	const clear = useWorkflowStore((s) => s.clear);
	const nodeCount = useWorkflowStore((s) => s.nodes.length);
	const pastLength = useWorkflowStore((s) => s.past.length);
	const futureLength = useWorkflowStore((s) => s.future.length);
	const currentFile = useWorkflowStore((s) => s.currentFile);
	const openModal = useUIStore((s) => s.openModal);

	const [savePopoverOpen, setSavePopoverOpen] = useState(false);
	const [saveName, setSaveName] = useState("");
	const [saveDesc, setSaveDesc] = useState("");
	const [runDialogOpen, setRunDialogOpen] = useState(false);
	const [runInputs, setRunInputs] = useState<WorkflowPort[]>([]);

	const handleRun = async () => {
		const workflow = useWorkflowStore.getState().exportWorkflow();
		const inputs = workflow.interface?.inputs ?? [];
		const workflowName = currentFile?.name ?? "Editor workflow";

		if (inputs.length > 0) {
			setRunInputs(inputs);
			setRunDialogOpen(true);
		} else {
			try {
				await useJobStore.getState().submitJob(workflow, { workflowName });
				toast.success(t("toolbar.toast.jobSubmitted"));
				void navigate("/jobs");
			} catch (err) {
				toast.error(
					err instanceof Error ? err.message : t("toolbar.toast.submitFailed"),
				);
			}
		}
	};

	const handleSave = async () => {
		if (currentFile) {
			try {
				const workflow = useWorkflowStore.getState().exportWorkflow();
				await saveWorkflow(currentFile.name, currentFile.description, workflow);
				toast.success(t("toolbar.toast.workflowSaved"));
			} catch {
				toast.error(t("toolbar.toast.saveFailed"));
			}
		} else {
			setSavePopoverOpen(true);
		}
	};

	const handleSaveConfirm = async () => {
		if (!saveName.trim()) return;
		try {
			const workflow = useWorkflowStore.getState().exportWorkflow();
			const entry = await saveWorkflow(
				saveName.trim(),
				saveDesc.trim(),
				workflow,
			);
			useWorkflowStore.getState().setCurrentFile({
				filename: entry.filename,
				name: entry.name,
				description: entry.description,
			});
			toast.success(t("toolbar.toast.workflowSaved"));
			setSavePopoverOpen(false);
			setSaveName("");
			setSaveDesc("");
		} catch {
			toast.error(t("toolbar.toast.saveFailed"));
		}
	};

	const handleDownload = () => {
		const workflow = useWorkflowStore.getState().exportWorkflow();
		const json = JSON.stringify(workflow, null, 2);
		const blob = new Blob([json], { type: "application/json" });
		const url = URL.createObjectURL(blob);
		const a = document.createElement("a");
		a.href = url;
		a.download = currentFile?.filename ?? "workflow.json";
		a.click();
		URL.revokeObjectURL(url);
		toast.success(t("toolbar.toast.workflowDownloaded"));
	};

	return (
		<>
			<div className="absolute top-3 left-1/2 -translate-x-1/2 z-10 flex items-center gap-1 bg-card/90 backdrop-blur-md border border-border/50 rounded-lg px-2 py-1 shadow-lg">
				<ToolbarButton
					icon={Undo2}
					label={t("toolbar.undo")}
					onClick={undo}
					disabled={pastLength === 0}
				/>
				<ToolbarButton
					icon={Redo2}
					label={t("toolbar.redo")}
					onClick={redo}
					disabled={futureLength === 0}
				/>

				<div className="w-px h-5 bg-border/50 mx-1" />

				<ToolbarButton
					icon={LayoutDashboard}
					label={t("toolbar.autoLayout")}
					onClick={onAutoLayout}
				/>
				<ToolbarButton
					icon={Maximize}
					label={t("toolbar.fitView")}
					onClick={onFitView}
				/>

				<div className="w-px h-5 bg-border/50 mx-1" />

				<ToolbarButton
					icon={Trash2}
					label={t("toolbar.clear")}
					onClick={clear}
				/>

				<Popover open={savePopoverOpen} onOpenChange={setSavePopoverOpen}>
					<PopoverTrigger asChild>
						<div>
							<ToolbarButton
								icon={Save}
								label={t("toolbar.save")}
								onClick={() => {
									void handleSave();
								}}
								disabled={nodeCount === 0}
							/>
						</div>
					</PopoverTrigger>
					<PopoverContent className="w-72" side="bottom" align="center">
						<div className="space-y-2">
							<p className="text-sm font-medium">
								{t("toolbar.saveDialog.title")}
							</p>
							<Input
								placeholder={t("toolbar.saveDialog.namePlaceholder")}
								value={saveName}
								onChange={(e) => {
									setSaveName(e.target.value);
								}}
								className="text-sm"
							/>
							<Input
								placeholder={t("toolbar.saveDialog.descriptionPlaceholder")}
								value={saveDesc}
								onChange={(e) => {
									setSaveDesc(e.target.value);
								}}
								className="text-sm"
							/>
							<div className="flex gap-2 justify-end">
								<Button
									variant="ghost"
									size="sm"
									onClick={() => {
										setSavePopoverOpen(false);
									}}
								>
									{t("toolbar.saveDialog.cancel")}
								</Button>
								<Button
									size="sm"
									onClick={() => {
										void handleSaveConfirm();
									}}
									disabled={!saveName.trim()}
								>
									{t("toolbar.saveDialog.confirm")}
								</Button>
							</div>
						</div>
					</PopoverContent>
				</Popover>

				<ToolbarButton
					icon={Download}
					label={t("toolbar.download")}
					onClick={handleDownload}
					disabled={nodeCount === 0}
				/>
				<ToolbarButton
					icon={FolderOpen}
					label={t("toolbar.load")}
					onClick={() => {
						openModal("presets");
					}}
				/>

				<div className="w-px h-5 bg-border/50 mx-1" />

				<ToolbarButton
					icon={Play}
					label={t("toolbar.runWorkflow")}
					onClick={() => {
						void handleRun();
					}}
					disabled={nodeCount === 0}
				/>
				<ToolbarButton
					icon={FolderInput}
					label={t("toolbar.batchProcessing")}
					onClick={() => {
						openModal("batch");
					}}
					disabled={nodeCount === 0}
				/>
				<ToolbarButton
					icon={Eye}
					label={t("toolbar.preview")}
					onClick={() => {
						openModal("preview");
					}}
				/>
			</div>
			<RunFromEditorDialog
				open={runDialogOpen}
				onOpenChange={setRunDialogOpen}
				inputs={runInputs}
			/>
		</>
	);
}
