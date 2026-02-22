import { CheckCircle2, Loader2, Send, Workflow } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router";
import { submitJob, submitJobWithParams } from "@/api/client";
import { convertParam } from "@/components/shared/port-field-utils";
import { toast } from "@/components/shared/Toaster";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogFooter,
	DialogHeader,
	DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Separator } from "@/components/ui/separator";
import { useUIStore } from "@/stores/ui-store";
import { useWorkflowStore } from "@/stores/workflow-store";
import type { BatchResponse, WorkflowPort } from "@/types";

const BatchMode = {
	WorkflowInputs: "workflow_inputs",
	RepeatCount: "repeat_count",
} as const;

type BatchMode = (typeof BatchMode)[keyof typeof BatchMode];

interface BatchDraftState {
	mode: BatchMode;
	workflowInputs: Record<string, string>;
	repeatCount: string;
}

const REPEAT_COUNT_MIN = 1;
const REPEAT_COUNT_MAX = 1000;
const REPEAT_COUNT_DEFAULT = "1";
const ROW_COUNT_MIN = 1;
const ROW_COUNT_MAX = 1000;

type RepeatCountErrorKey =
	| "batch.errors.repeatCount.required"
	| "batch.errors.repeatCount.invalidType"
	| "batch.errors.repeatCount.tooSmall"
	| "batch.errors.repeatCount.tooLarge";

type RepeatCountValidation =
	| {
			ok: true;
			value: number;
	  }
	| {
			ok: false;
			errorKey: RepeatCountErrorKey;
	  };

interface RepeatSubmissionPlanMetadata {
	mode: typeof BatchMode.RepeatCount;
	repeatCount: number;
	templateRowIndex: number;
	totalSubmissions: number;
	submissionOrder: number[];
	planFingerprint: string;
}

interface WorkflowRowIssue {
	portName: string;
	rowNumber: number;
	reason: string;
}

interface WorkflowRowValidationResult {
	rowMatrix: Array<Record<string, string>>;
	rowCount: number;
	mismatchCount: number;
	requiredCount: number;
	typeInvalidCount: number;
	issues: WorkflowRowIssue[];
	isValid: boolean;
}

function validateRepeatCount(rawValue: string): RepeatCountValidation {
	const value = rawValue.trim();
	if (value.length === 0) {
		return { ok: false, errorKey: "batch.errors.repeatCount.required" };
	}
	if (!/^\d+$/.test(value)) {
		return { ok: false, errorKey: "batch.errors.repeatCount.invalidType" };
	}
	const parsed = Number(value);
	if (!Number.isSafeInteger(parsed)) {
		return { ok: false, errorKey: "batch.errors.repeatCount.invalidType" };
	}
	if (parsed < REPEAT_COUNT_MIN) {
		return { ok: false, errorKey: "batch.errors.repeatCount.tooSmall" };
	}
	if (parsed > REPEAT_COUNT_MAX) {
		return { ok: false, errorKey: "batch.errors.repeatCount.tooLarge" };
	}
	return { ok: true, value: parsed };
}

function buildRepeatSubmissionPlanMetadata(
	repeatCount: number,
): RepeatSubmissionPlanMetadata {
	const submissionOrder = Array.from({ length: repeatCount }, (_, index) => index + 1);
	return {
		mode: BatchMode.RepeatCount,
		repeatCount,
		templateRowIndex: 0,
		totalSubmissions: submissionOrder.length,
		submissionOrder,
		planFingerprint: `repeat_count:row-0:x${repeatCount}`,
	};
}

function normalizeRows(value: string): string[] {
	const normalized = value.replace(/\r\n/g, "\n").replace(/\r/g, "\n");
	const rows = normalized.split("\n");
	if (rows.at(-1) === "") {
		rows.pop();
	}
	return rows;
}

function isValidTypedCell(port: WorkflowPort, rawValue: string): boolean {
	const trimmed = rawValue.trim();
	if (!trimmed) return true;

	if (port.port_type === "Int" || port.port_type === "Float") {
		const converted = convertParam(port, trimmed);
		return typeof converted === "number" && !Number.isNaN(converted);
	}

	if (port.port_type === "Bool") {
		const normalized = trimmed.toLowerCase();
		return normalized === "true" || normalized === "false";
	}

	return true;
}

function formatRowIssue(issue: WorkflowRowIssue): string {
	return `${issue.portName} row ${issue.rowNumber}: ${issue.reason}`;
}

function validateWorkflowRows(
	inputs: WorkflowPort[],
	workflowInputs: Record<string, string>,
): WorkflowRowValidationResult {
	const normalizedByPort = inputs.map((port) => ({
		port,
		rows: normalizeRows(workflowInputs[port.name] ?? ""),
	}));
	const referenceRowCount = normalizedByPort[0]?.rows.length ?? 0;

	const issues: WorkflowRowIssue[] = [];
	let mismatchCount = 0;
	let requiredCount = 0;
	let typeInvalidCount = 0;

	for (const { port, rows } of normalizedByPort) {
		if (rows.length === referenceRowCount) continue;
		mismatchCount += 1;
		issues.push({
			portName: port.name,
			rowNumber: Math.min(rows.length, referenceRowCount) + 1,
			reason: `line count mismatch (expected ${referenceRowCount}, got ${rows.length})`,
		});
	}

	if (referenceRowCount < ROW_COUNT_MIN || referenceRowCount > ROW_COUNT_MAX) {
		issues.push({
			portName: normalizedByPort[0]?.port.name ?? "rows",
			rowNumber: referenceRowCount < ROW_COUNT_MIN ? 1 : ROW_COUNT_MAX + 1,
			reason: `row count ${referenceRowCount} is out of bounds [${ROW_COUNT_MIN}, ${ROW_COUNT_MAX}]`,
		});
	}

	const hasStrictRowCount =
		referenceRowCount >= ROW_COUNT_MIN && referenceRowCount <= ROW_COUNT_MAX;
	const canBuildMatrix = mismatchCount === 0 && hasStrictRowCount;

	const rowMatrix = canBuildMatrix
		? Array.from({ length: referenceRowCount }, (_, rowIndex) =>
				Object.fromEntries(
					normalizedByPort.map(({ port, rows }) => [port.name, rows[rowIndex] ?? ""]),
				),
			)
		: [];

	if (canBuildMatrix) {
		for (let rowIndex = 0; rowIndex < rowMatrix.length; rowIndex += 1) {
			const row = rowMatrix[rowIndex];
			for (const port of inputs) {
				const value = row[port.name] ?? "";
				if (port.default_value === undefined && value.trim().length === 0) {
					requiredCount += 1;
					issues.push({
						portName: port.name,
						rowNumber: rowIndex + 1,
						reason: "required value is blank",
					});
					continue;
				}

				if (!isValidTypedCell(port, value)) {
					typeInvalidCount += 1;
					issues.push({
						portName: port.name,
						rowNumber: rowIndex + 1,
						reason: `invalid ${port.port_type} value "${value}"`,
					});
				}
			}
		}
	}

	return {
		rowMatrix,
		rowCount: referenceRowCount,
		mismatchCount,
		requiredCount,
		typeInvalidCount,
		issues,
		isValid: issues.length === 0,
	};
}

function detectBatchMode(inputs: WorkflowPort[]): BatchMode {
	return inputs.length > 0 ? BatchMode.WorkflowInputs : BatchMode.RepeatCount;
}

function buildParamsForRow(
	inputs: WorkflowPort[],
	row: Record<string, string>,
): Record<string, string | number | boolean> {
	return Object.fromEntries(
		inputs.map((port) => {
			const rawRowValue = row[port.name] ?? "";
			const trimmedRowValue = rawRowValue.trim();

			const resolvedValue =
				trimmedRowValue.length === 0 && port.default_value !== undefined
					? String(port.default_value)
					: rawRowValue;

			if (port.port_type === "Bool") {
				if (typeof resolvedValue === "boolean") {
					return [port.name, resolvedValue];
				}
				const normalized = String(resolvedValue).trim().toLowerCase();
				if (normalized === "true") {
					return [port.name, true];
				}
				if (normalized === "false") {
					return [port.name, false];
				}
			}

			return [port.name, convertParam(port, resolvedValue)];
		}),
	);
}

function buildWorkflowInputTextMap(
	inputs: WorkflowPort[],
	previous: Record<string, string> = {},
): Record<string, string> {
	return Object.fromEntries(
		inputs.map((port) => [port.name, previous[port.name] ?? ""]),
	);
}

export function BatchPanel() {
	const { t } = useTranslation("settings");
	const activeModal = useUIStore((s) => s.activeModal);
	const closeModal = useUIStore((s) => s.closeModal);
	const open = activeModal === "batch";

	const workflowNodes = useWorkflowStore((s) => s.nodes);
	const exportWorkflow = useWorkflowStore((s) => s.exportWorkflow);
	const nodeCount = workflowNodes.length;
	const workflowInputs = useMemo(
		() => {
			if (workflowNodes.length === 0) return [];
			return exportWorkflow().interface?.inputs ?? [];
		},
		[exportWorkflow, workflowNodes],
	);
	const detectedMode = useMemo(() => detectBatchMode(workflowInputs), [workflowInputs]);

	const [batchDraft, setBatchDraft] = useState<BatchDraftState>(() => ({
		mode: detectedMode,
		workflowInputs: buildWorkflowInputTextMap(workflowInputs),
		repeatCount: REPEAT_COUNT_DEFAULT,
	}));
	const [submitting, setSubmitting] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const [result, setResult] = useState<BatchResponse | null>(null);
	const repeatPlanMetadataRef = useRef<RepeatSubmissionPlanMetadata | null>(null);

	useEffect(() => {
		setBatchDraft((prev) => ({
			...prev,
			mode: detectedMode,
			workflowInputs: buildWorkflowInputTextMap(workflowInputs, prev.workflowInputs),
		}));
	}, [detectedMode, workflowInputs]);

	const rowValidation = useMemo(
		() => validateWorkflowRows(workflowInputs, batchDraft.workflowInputs),
		[batchDraft.workflowInputs, workflowInputs],
	);

	const canSubmit =
		batchDraft.mode === BatchMode.WorkflowInputs
			? true
			: validateRepeatCount(batchDraft.repeatCount).ok;

	const repeatCountValidation = useMemo(
		() => validateRepeatCount(batchDraft.repeatCount),
		[batchDraft.repeatCount],
	);
	const repeatPlanMetadata = useMemo(() => {
		if (batchDraft.mode !== BatchMode.RepeatCount || !repeatCountValidation.ok) {
			return null;
		}
		return buildRepeatSubmissionPlanMetadata(repeatCountValidation.value);
	}, [batchDraft.mode, repeatCountValidation]);
	const repeatCountValidationError =
		batchDraft.mode === BatchMode.RepeatCount && !repeatCountValidation.ok
			? t(repeatCountValidation.errorKey, {
					min: REPEAT_COUNT_MIN,
					max: REPEAT_COUNT_MAX,
			  })
			: null;
	const displayError = repeatCountValidationError ?? error;

	const buildRowValidationErrorMessage = useCallback(
		(validation: WorkflowRowValidationResult) => {
			const lines: string[] = [];

			if (validation.mismatchCount > 0) {
				lines.push(
					t("batch.errors.mismatchSummary", { count: validation.mismatchCount }),
				);
			}

			if (validation.requiredCount > 0) {
				lines.push(
					t("batch.validation.requiredSummary", { count: validation.requiredCount }),
				);
			}

			if (validation.typeInvalidCount > 0) {
				lines.push(
					t("batch.errors.typeInvalidSummary", {
						count: validation.typeInvalidCount,
					}),
				);
			}

			lines.push(...validation.issues.map(formatRowIssue));
			lines.push(t("batch.errors.failFastSummary"));

			return lines.join("\n");
		},
		[t],
	);

	const handleWorkflowInputChange = useCallback((portName: string, value: string) => {
		setError(null);
		setBatchDraft((prev) => ({
			...prev,
			workflowInputs: {
				...prev.workflowInputs,
				[portName]: value,
			},
		}));
	}, []);

	const handleSubmit = useCallback(async () => {
		if (submitting) return;

		let totalPlannedSubmissions = 0;
		let workflowInputRows: Array<Record<string, string>> = [];

		if (batchDraft.mode === BatchMode.RepeatCount) {
			if (!repeatCountValidation.ok) {
				setError(
					t(repeatCountValidation.errorKey, {
						min: REPEAT_COUNT_MIN,
						max: REPEAT_COUNT_MAX,
					}),
				);
				return;
			}
			if (!repeatPlanMetadata) {
				setError(t("batch.errors.repeatCount.invalidType"));
				return;
			}
			repeatPlanMetadataRef.current = repeatPlanMetadata;
			totalPlannedSubmissions = repeatPlanMetadata.totalSubmissions;
		} else {
			if (!rowValidation.isValid) {
				setError(buildRowValidationErrorMessage(rowValidation));
				return;
			}

			workflowInputRows = rowValidation.rowMatrix;
			totalPlannedSubmissions = workflowInputRows.length;
		}

		if (totalPlannedSubmissions <= 0) {
			setError(t("batch.errors.submitFailed"));
			return;
		}

		setSubmitting(true);
		setError(null);
		setResult(null);

		const submittedJobIds: string[] = [];
		try {
			const workflow = useWorkflowStore.getState().exportWorkflow();

			if (batchDraft.mode === BatchMode.RepeatCount) {
				for (let index = 0; index < totalPlannedSubmissions; index += 1) {
					const response = await submitJob(workflow);
					submittedJobIds.push(response.id);
				}
			} else {
				for (const row of workflowInputRows) {
					const rowParams = buildParamsForRow(workflowInputs, row);
					const response = await submitJobWithParams(workflow, rowParams);
					submittedJobIds.push(response.id);
				}
			}

			setResult({
				job_ids: submittedJobIds,
				total: submittedJobIds.length,
			});
			toast.success(t("batch.toast.submitted", { count: submittedJobIds.length }));
			setBatchDraft((prev) => ({
				...prev,
				workflowInputs: buildWorkflowInputTextMap(workflowInputs),
			}));
		} catch (err) {
			const partialProgressLine = `submitted ${submittedJobIds.length} / total ${totalPlannedSubmissions}`;
			setError(
				`${err instanceof Error ? err.message : t("batch.errors.submitFailed")}${"\n"}${partialProgressLine}`,
			);
		} finally {
			setSubmitting(false);
		}
	}, [
		batchDraft.mode,
		buildRowValidationErrorMessage,
		rowValidation,
		repeatCountValidation,
		repeatPlanMetadata,
		submitting,
		t,
		workflowInputs,
	]);

	const handleOpenChange = useCallback(
		(nextOpen: boolean) => {
			if (!nextOpen) {
				closeModal();
				setResult(null);
				setError(null);
				repeatPlanMetadataRef.current = null;
			}
		},
		[closeModal],
	);

	return (
		<Dialog open={open} onOpenChange={handleOpenChange}>
			<DialogContent className="max-w-xl">
				<DialogHeader>
					<DialogTitle>{t("batch.title")}</DialogTitle>
					<DialogDescription>{t("batch.description")}</DialogDescription>
				</DialogHeader>

				<div className="space-y-4">
					<div className="flex items-center gap-2 text-xs text-muted-foreground">
						<Workflow className="h-3.5 w-3.5" />
						<span>
							{t("batch.workflow.current")}{" "}
							<strong className="text-foreground">
								{t("batch.workflow.nodes", { count: nodeCount })}
							</strong>
						</span>
					</div>

					<Separator />

					<div className="flex items-center justify-between gap-2 rounded-md border border-border/60 bg-secondary/20 px-3 py-2">
						<span className="text-xs font-medium text-muted-foreground">
							{t("batch.mode.label")}
						</span>
						<Badge variant="secondary" className="text-[10px] uppercase tracking-wide">
							{batchDraft.mode === BatchMode.WorkflowInputs
								? t("batch.mode.rowwise")
								: t("batch.mode.templateRepeat")}
						</Badge>
					</div>

					{batchDraft.mode === BatchMode.WorkflowInputs ? (
						<div className="space-y-3">
							{workflowInputs.map((port) => (
								<div key={port.name} className="space-y-2">
									<label
										htmlFor={`batch-input-${port.name}`}
										className="text-sm font-medium text-foreground"
									>
										{t("batch.inputs.perPortMultilineLabel", { port: port.name })}
									</label>
									<textarea
										id={`batch-input-${port.name}`}
										value={batchDraft.workflowInputs[port.name] ?? ""}
										onChange={(event) => {
											handleWorkflowInputChange(port.name, event.target.value);
										}}
										placeholder={t("batch.inputs.perPortMultilinePlaceholder")}
										rows={4}
										className="flex w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm font-mono shadow-sm placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring resize-y min-h-[80px]"
									/>
								</div>
							))}
						</div>
					) : (
						<div className="space-y-2">
							<label
								htmlFor="batch-repeat-count"
								className="text-sm font-medium text-foreground"
							>
								{t("batch.inputs.repeatCountLabel")}
							</label>
							<Input
								id="batch-repeat-count"
								type="number"
								min={REPEAT_COUNT_MIN}
								max={REPEAT_COUNT_MAX}
								step={1}
								value={batchDraft.repeatCount}
								onChange={(event) => {
									setBatchDraft((prev) => ({
										...prev,
										repeatCount: event.target.value,
									}));
									setError(null);
								}}
								placeholder={t("batch.inputs.repeatCountPlaceholder")}
								aria-invalid={batchDraft.mode === BatchMode.RepeatCount && !repeatCountValidation.ok}
							/>
							<p className="text-xs text-muted-foreground">
								{t("batch.inputs.repeatCountHelp")}
							</p>
						</div>
					)}

					{batchDraft.mode === BatchMode.WorkflowInputs &&
						rowValidation.rowCount > 0 &&
						rowValidation.mismatchCount === 0 && (
						<p className="text-xs text-muted-foreground">
							{t("batch.count.paths", { count: rowValidation.rowMatrix.length })}
						</p>
					)}

					{displayError && (
						<p className="text-xs text-destructive whitespace-pre-line">
							{displayError}
						</p>
					)}

					{result && (
						<div className="rounded-lg border border-green-500/30 bg-green-500/5 p-4 space-y-3">
							<div className="flex items-center gap-2 text-sm text-green-400">
								<CheckCircle2 className="h-4 w-4" />
								<span className="font-medium">
									{t("batch.count.submitted", { count: result.total })}
								</span>
							</div>
							<div className="flex flex-wrap gap-1.5">
								{result.job_ids.map((id) => (
									<Badge
										key={id}
										variant="secondary"
										className="font-mono text-[10px]"
									>
										{id.slice(0, 8)}
									</Badge>
								))}
							</div>
							<Link
								to="/jobs"
								onClick={() => closeModal()}
								className="inline-block text-xs text-primary hover:underline"
							>
								{t("batch.actions.viewJobsPage")}
							</Link>
						</div>
					)}
				</div>

				<DialogFooter>
					<Button variant="secondary" onClick={() => closeModal()}>
						{t("batch.actions.close")}
					</Button>
					<Button
						onClick={() => void handleSubmit()}
						disabled={submitting || !canSubmit}
					>
						{submitting ? (
							<Loader2 className="h-3.5 w-3.5 animate-spin" />
						) : (
							<Send className="h-3.5 w-3.5" />
						)}
						{t("batch.actions.submit")}
					</Button>
				</DialogFooter>
			</DialogContent>
		</Dialog>
	);
}
