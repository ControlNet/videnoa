import {
	Activity,
	ChevronDown,
	ChevronRight,
	Clock,
	Gauge,
	Loader2,
	Play,
	RotateCcw,
	Timer,
	Trash2,
	X,
} from "lucide-react";
import type { ReactNode } from "react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { PageContainer } from "@/components/layout/PageContainer";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import { Separator } from "@/components/ui/separator";
import {
	formatLocaleDateTime,
	formatRelativeTime,
} from "@/lib/presentation-format";
import { useJobStore } from "@/stores/job-store";
import type { Job, JobStatus, NodeRuntimePreview } from "@/types";
import { RunWorkflowDialog } from "./RunWorkflowDialog";
import { formatDuration, formatETA } from "./time-utils";

// ─── Helpers ──────────────────────────────────────────────────────────────────

const STATUS_STYLES: Record<JobStatus, string> = {
	queued: "bg-yellow-500/20 text-yellow-400 border-yellow-500/30",
	running: "bg-blue-500/20 text-blue-400 border-blue-500/30",
	completed: "bg-green-500/20 text-green-400 border-green-500/30",
	failed: "bg-red-500/20 text-red-400 border-red-500/30",
	cancelled: "bg-gray-500/20 text-gray-400 border-gray-500/30",
};

function getParamsSummary(
	params: Job["params"],
	t: (key: string, options?: Record<string, unknown>) => string,
): string {
	if (!params || typeof params !== "object" || Array.isArray(params)) {
		return t("jobs.page.history.params.empty");
	}

	const count = Object.keys(params).length;
	if (count === 0) {
		return t("jobs.page.history.params.empty");
	}

	return t("jobs.page.history.params.summary", { count });
}

function getParamsJson(params: Job["params"]): string {
	try {
		return JSON.stringify(params ?? null, null, 2) ?? "null";
	} catch {
		return "null";
	}
}

// ─── Active Job Card ──────────────────────────────────────────────────────────

function ActiveJobCard({ job }: { job: Job }) {
	const { t } = useTranslation("jobs");
	const { activeProgress, cancelJob } = useJobStore();
	const [cancelling, setCancelling] = useState(false);

	const progress = activeProgress ?? job.progress;
	const totalFrames = progress?.total_frames ?? null;
	const currentFrame = progress?.current_frame ?? 0;
	const inputFps =
		typeof progress?.fps === "number" && Number.isFinite(progress.fps)
			? progress.fps
			: 0;
	const etaDisplay =
		typeof progress?.eta_seconds === "number" && Number.isFinite(progress.eta_seconds)
			? progress.eta_seconds > 0
				? formatETA(progress.eta_seconds)
				: "00:00:00"
			: "00:00:00";
	const percentage =
		totalFrames != null && totalFrames > 0
			? Math.min(Math.round((currentFrame / totalFrames) * 100), 100)
			: 0;

	const handleCancel = useCallback(async () => {
		setCancelling(true);
		try {
			await cancelJob(job.id);
		} catch {
			// store handles error state
		} finally {
			setCancelling(false);
		}
	}, [cancelJob, job.id]);

	return (
		<Card className="border-blue-500/30 bg-blue-500/5">
			<CardHeader className="pb-3">
				<div className="flex items-center justify-between">
					<div className="flex items-center gap-3">
						<div className="flex h-8 w-8 items-center justify-center rounded-lg bg-blue-500/20">
							<Activity className="h-4 w-4 text-blue-400" />
						</div>
						<div>
							<CardTitle className="text-base">
								{t("jobs.page.active.title")}
							</CardTitle>
							<p className="text-xs text-muted-foreground font-mono mt-0.5">
								{job.id.slice(0, 8)}
							</p>
						</div>
					</div>
					<Button
						variant="destructive"
						size="sm"
						onClick={() => void handleCancel()}
						disabled={cancelling}
					>
						{cancelling ? (
							<Loader2 className="h-3.5 w-3.5 animate-spin" />
						) : (
							<X className="h-3.5 w-3.5" />
						)}
						{t("jobs.page.actions.cancel")}
					</Button>
				</div>
			</CardHeader>
			<CardContent className="space-y-4">
				{/* Progress bar */}
				<div className="space-y-2">
					<Progress value={percentage} className="h-3" />
					<div className="flex items-center justify-between text-xs text-muted-foreground">
						<span className="font-semibold text-foreground text-sm">
							{String(percentage)}%
						</span>
						{totalFrames != null && (
							<span>
								{t("jobs.page.active.progressFrames", {
									current: currentFrame,
									total: totalFrames,
								})}
							</span>
						)}
					</div>
				</div>

				{/* Stats row */}
				<div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
					<StatItem
						icon={<Gauge className="h-3.5 w-3.5" />}
						label={t("jobs.page.active.stats.inputFps")}
						value={inputFps.toFixed(1)}
						testId="jobs-active-stat-input-fps"
					/>
					<StatItem
						icon={<Timer className="h-3.5 w-3.5" />}
						label={t("jobs.page.active.stats.eta")}
						value={etaDisplay}
						testId="jobs-active-stat-eta"
					/>
					<StatItem
						icon={<Clock className="h-3.5 w-3.5" />}
						label={t("jobs.page.active.stats.elapsed")}
						value={job.started_at ? formatDuration(job.started_at, null) : "—"}
					/>
					<StatItem
						icon={<Activity className="h-3.5 w-3.5" />}
						label={t("jobs.page.active.stats.status")}
						value={t(`jobs.status.${job.status}`)}
					/>
				</div>
			</CardContent>
		</Card>
	);
}

function StatItem({
	icon,
	label,
	value,
	testId,
}: {
	icon: ReactNode;
	label: string;
	value: string;
	testId?: string;
}) {
	return (
		<div className="flex items-start gap-2 rounded-lg bg-secondary/50 px-3 py-2">
			<span className="mt-0.5 text-muted-foreground">{icon}</span>
			<div className="min-w-0">
				<p className="text-[10px] uppercase tracking-wider text-muted-foreground">
					{label}
				</p>
				<p data-testid={testId} className="text-sm font-medium truncate">
					{value}
				</p>
			</div>
		</div>
	);
}

// ─── Job History Row ──────────────────────────────────────────────────────────

function JobRow({
	job,
	printRuntimePreviews,
}: {
	job: Job;
	printRuntimePreviews: NodeRuntimePreview[];
}) {
	const { t } = useTranslation("jobs");
	const { rerunJob, deleteJobHistory } = useJobStore();
	const [expanded, setExpanded] = useState(false);
	const [retrying, setRetrying] = useState(false);
	const [deleting, setDeleting] = useState(false);

	const paramsSummary = getParamsSummary(job.params, t);
	const isCompleted = job.status === "completed";
	const actionsDisabled = retrying || deleting;

	const handleRetry = useCallback(async () => {
		setRetrying(true);
		try {
			await rerunJob(job.id);
		} catch {
			// store handles error state
		} finally {
			setRetrying(false);
		}
	}, [job.id, rerunJob]);

	const handleDelete = useCallback(async () => {
		setDeleting(true);
		try {
			await deleteJobHistory(job.id);
		} catch {
			// store handles error state
		} finally {
			setDeleting(false);
		}
	}, [deleteJobHistory, job.id]);

	return (
		<div className="group">
			<div className="flex w-full items-center px-4 py-2 text-sm transition-colors hover:bg-secondary/40">
				<button
					type="button"
					className="flex min-w-0 flex-1 items-center gap-4 py-1 text-left"
					onClick={() => setExpanded((v) => !v)}
				>
					{/* Expand chevron */}
					<span className="text-muted-foreground">
						{expanded ? (
							<ChevronDown className="h-3.5 w-3.5" />
						) : (
							<ChevronRight className="h-3.5 w-3.5" />
						)}
					</span>

					{/* ID */}
					<span className="w-20 shrink-0 font-mono text-xs text-muted-foreground">
						{job.id.slice(0, 8)}
					</span>

					{/* Status badge */}
					<span className="w-24 shrink-0">
						<Badge variant="outline" className={STATUS_STYLES[job.status]}>
							{t(`jobs.status.${job.status}`)}
						</Badge>
					</span>

					<span className="w-36 shrink-0 truncate text-xs text-muted-foreground">
						{job.workflow_name || "—"}
					</span>

					{/* Created */}
					<span className="w-28 shrink-0 text-xs text-muted-foreground">
						{formatRelativeTime(job.created_at)}
					</span>

					{/* Duration */}
					<span className="w-24 shrink-0 text-xs text-muted-foreground">
						{job.started_at
							? formatDuration(job.started_at, job.completed_at)
							: "—"}
					</span>

					<span className="w-24 shrink-0 truncate text-xs text-muted-foreground">
						{paramsSummary}
					</span>

					{/* Error preview */}
					<span className="min-w-0 flex-1 truncate text-xs text-red-400/80">
						{job.error ? job.error.slice(0, 60) : ""}
					</span>
				</button>

				<div className="flex w-36 shrink-0 items-center justify-end gap-1 pl-3">
					{!isCompleted && (
						<Button
							type="button"
							variant="outline"
							size="sm"
							disabled={actionsDisabled}
							onClick={(event) => {
								event.stopPropagation();
								void handleRetry();
							}}
						>
							{retrying ? (
								<Loader2 className="h-3.5 w-3.5 animate-spin" />
							) : (
								<RotateCcw className="h-3.5 w-3.5" />
							)}
							{t("jobs.page.actions.retry")}
						</Button>
					)}
					<Button
						type="button"
						variant="outline"
						size="sm"
						disabled={actionsDisabled}
						onClick={(event) => {
							event.stopPropagation();
							void handleDelete();
						}}
					>
						{deleting ? (
							<Loader2 className="h-3.5 w-3.5 animate-spin" />
						) : (
							<Trash2 className="h-3.5 w-3.5" />
						)}
						{t("jobs.page.actions.delete")}
					</Button>
				</div>
			</div>

			{/* Expanded details */}
			{expanded && (
				<div className="border-t border-border/50 bg-secondary/20 px-4 py-3 pl-12">
					<div className="grid grid-cols-2 gap-x-8 gap-y-2 text-xs">
						<div>
							<span className="text-muted-foreground">
								{t("jobs.page.history.details.fullId")}
							</span>{" "}
							<span className="font-mono">{job.id}</span>
						</div>
						<div>
							<span className="text-muted-foreground">
								{t("jobs.page.history.details.status")}
							</span>{" "}
							<span>{t(`jobs.status.${job.status}`)}</span>
						</div>
						<div>
							<span className="text-muted-foreground">
								{t("jobs.page.history.details.workflowName")}
							</span>{" "}
							<span>{job.workflow_name || "—"}</span>
						</div>
						<div>
							<span className="text-muted-foreground">
								{t("jobs.page.history.details.workflowSource")}
							</span>{" "}
							<span className="font-mono">{job.workflow_source || "—"}</span>
						</div>
						<div>
							<span className="text-muted-foreground">
								{t("jobs.page.history.details.created")}
							</span>{" "}
							<span>{formatLocaleDateTime(job.created_at)}</span>
						</div>
						<div>
							<span className="text-muted-foreground">
								{t("jobs.page.history.details.started")}
							</span>{" "}
							<span>
								{job.started_at ? formatLocaleDateTime(job.started_at) : "—"}
							</span>
						</div>
						<div>
							<span className="text-muted-foreground">
								{t("jobs.page.history.details.completed")}
							</span>{" "}
							<span>
								{job.completed_at
									? formatLocaleDateTime(job.completed_at)
									: "—"}
							</span>
						</div>
						<div>
							<span className="text-muted-foreground">
								{t("jobs.page.history.details.duration")}
							</span>{" "}
							<span>
								{job.started_at
									? formatDuration(job.started_at, job.completed_at)
									: "—"}
							</span>
						</div>
						<div className="col-span-2">
							<p className="text-muted-foreground">
								{t("jobs.page.history.details.params")}
							</p>
							<pre className="mt-1 rounded-md border border-border/60 bg-background/50 px-3 py-2 whitespace-pre-wrap break-all font-mono text-xs text-foreground">
								{getParamsJson(job.params)}
							</pre>
						</div>
						{job.error && (
							<div className="col-span-2">
								<span className="text-muted-foreground">
									{t("jobs.page.history.details.error")}
								</span>{" "}
								<span className="text-red-400">{job.error}</span>
							</div>
						)}
						{printRuntimePreviews.length > 0 && (
							<div className="col-span-2 mt-2 space-y-2">
								<p className="text-muted-foreground">Print output</p>
								<div className="space-y-2">
									{printRuntimePreviews.map((preview) => (
										<div
											key={`${preview.node_id}-${preview.updated_at_ms}`}
											className="rounded-md border border-border/60 bg-background/50 px-3 py-2"
										>
											<p className="font-mono text-[10px] text-muted-foreground">
												{preview.node_id}
											</p>
											<pre className="mt-1 whitespace-pre-wrap break-words font-mono text-xs text-foreground">
												{preview.value_preview}
												{preview.truncated ? "…" : ""}
											</pre>
										</div>
									))}
								</div>
							</div>
						)}
					</div>
				</div>
			)}
		</div>
	);
}

// ─── Empty State ──────────────────────────────────────────────────────────────

function EmptyState() {
	const { t } = useTranslation("jobs");

	return (
		<div className="flex flex-col items-center justify-center py-20 text-center">
			<div className="flex h-14 w-14 items-center justify-center rounded-2xl bg-secondary/60 mb-4">
				<Play className="h-6 w-6 text-muted-foreground" />
			</div>
			<h3 className="text-sm font-medium text-foreground mb-1">
				{t("jobs.page.empty.title")}
			</h3>
			<p className="text-xs text-muted-foreground max-w-[260px]">
				{t("jobs.page.empty.description")}
			</p>
		</div>
	);
}

// ─── Page ─────────────────────────────────────────────────────────────────────

export function JobsPage() {
	const { t } = useTranslation("jobs");
	const {
		jobs,
		fetchJobs,
		activeJobId,
		subscribeToJob,
		unsubscribeFromJob,
		runtimePreviewsByJobId,
	} = useJobStore();
	const [runDialogOpen, setRunDialogOpen] = useState(false);

	useEffect(() => {
		void fetchJobs();
		const id = setInterval(() => void fetchJobs(), 2_000);
		return () => clearInterval(id);
	}, [fetchJobs]);

	const trackedJob =
		jobs.find((job) => job.status === "running") ??
		jobs.find((job) => job.status === "queued") ??
		null;

	useEffect(() => {
		if (!trackedJob) {
			if (activeJobId !== null) {
				unsubscribeFromJob();
			}
			return;
		}

		if (activeJobId !== trackedJob.id) {
			subscribeToJob(trackedJob.id);
		}
	}, [activeJobId, subscribeToJob, trackedJob, unsubscribeFromJob]);

	useEffect(() => {
		return () => {
			useJobStore.getState().unsubscribeFromJob();
		};
	}, []);

	const activeJob =
		jobs.find((job) => job.id === activeJobId) ??
		jobs.find((job) => job.status === "running" || job.status === "queued");

	const sortedJobs = [...jobs].sort(
		(a, b) =>
			new Date(b.created_at).getTime() - new Date(a.created_at).getTime(),
	);

	const hasJobs = jobs.length > 0;

	const getPrintRuntimePreviews = useCallback(
		(jobId: string): NodeRuntimePreview[] => {
			return Object.values(runtimePreviewsByJobId[jobId] ?? {})
				.filter((preview) => preview.node_type === "Print")
				.sort((a, b) => b.updated_at_ms - a.updated_at_ms);
		},
		[runtimePreviewsByJobId],
	);

	return (
		<PageContainer title={t("jobs.page.title")}>
			<div className="flex items-center justify-between -mt-4 mb-6">
				<p className="text-muted-foreground text-sm">
					{hasJobs
						? t("jobs.page.summary.total", { count: jobs.length })
						: t("jobs.page.summary.empty")}
				</p>
				<Button
					size="sm"
					onClick={() => {
						setRunDialogOpen(true);
					}}
				>
					<Play className="h-3.5 w-3.5" />
					{t("jobs.page.actions.runWorkflow")}
				</Button>
			</div>

			{!hasJobs && <EmptyState />}

			{hasJobs && (
				<div className="space-y-6">
					{/* Active job */}
					{activeJob &&
						(activeJob.status === "running" ||
							activeJob.status === "queued") && (
							<ActiveJobCard job={activeJob} />
						)}

					{/* History table */}
					<Card>
						<CardHeader className="pb-3">
							<CardTitle className="text-base">
								{t("jobs.page.history.title")}
							</CardTitle>
						</CardHeader>
						<CardContent className="p-0">
							{/* Table header */}
							<div className="flex items-center gap-4 border-b border-border/50 px-4 py-2 text-[10px] uppercase tracking-wider text-muted-foreground">
								<span className="w-3.5" /> {/* chevron space */}
								<span className="w-20 shrink-0">
									{t("jobs.page.history.columns.id")}
								</span>
								<span className="w-24 shrink-0">
									{t("jobs.page.history.columns.status")}
								</span>
								<span className="w-36 shrink-0">
									{t("jobs.page.history.columns.workflow")}
								</span>
								<span className="w-28 shrink-0">
									{t("jobs.page.history.columns.created")}
								</span>
								<span className="w-24 shrink-0">
									{t("jobs.page.history.columns.duration")}
								</span>
								<span className="w-24 shrink-0">
									{t("jobs.page.history.columns.params")}
								</span>
								<span className="min-w-0 flex-1">
									{t("jobs.page.history.columns.error")}
								</span>
								<span className="w-36 shrink-0 text-right">
									{t("jobs.page.history.columns.actions")}
								</span>
							</div>

							<Separator />

							{/* Rows */}
							<div className="max-h-[480px] overflow-y-auto">
								{sortedJobs.map((job) => (
									<JobRow
										key={job.id}
										job={job}
										printRuntimePreviews={getPrintRuntimePreviews(job.id)}
									/>
								))}
							</div>
						</CardContent>
					</Card>
				</div>
			)}
			<RunWorkflowDialog
				open={runDialogOpen}
				onOpenChange={setRunDialogOpen}
				onSubmitted={() => {
					void fetchJobs();
				}}
			/>
		</PageContainer>
	);
}
