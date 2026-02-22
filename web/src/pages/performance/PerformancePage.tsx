import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { CartesianGrid, Line, LineChart, Pie, PieChart, XAxis, YAxis } from "recharts";
import { getPerformanceExport, getPerformanceOverview } from "@/api/client";
import { PageContainer } from "@/components/layout/PageContainer";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";
import {
	type ChartConfig,
	ChartContainer,
	ChartTooltip,
	ChartTooltipContent,
} from "@/components/ui/chart";
import type {
	PerformanceExportResponse,
	PerformanceMetrics,
	PerformanceOverviewResponse,
	PerformanceSeriesPoint,
	PerformanceStatus,
} from "@/types";

import "./performance-colors.css";

type TranslateFn = (key: string, options?: Record<string, unknown>) => string;

const EM_DASH = "—";
const BYTE_UNITS = ["B", "KiB", "MiB", "GiB", "TiB"] as const;

const CPU_PERCENT_KEYS = ["cpu_util_percent", "cpu_usage_percent", "cpu_percent"] as const;
const RAM_USED_BYTES_KEYS = ["ram_used_bytes", "memory_used_bytes"] as const;
const RAM_TOTAL_BYTES_KEYS = ["ram_total_bytes", "memory_total_bytes"] as const;
const GPU_PERCENT_KEYS = ["gpu_util_percent", "gpu_usage_percent", "gpu_percent"] as const;
const VRAM_USED_BYTES_KEYS = [
	"vram_used_bytes",
	"gpu_mem_used_bytes",
	"gpu_memory_used_bytes",
] as const;
const VRAM_TOTAL_BYTES_KEYS = [
	"vram_total_bytes",
	"gpu_mem_total_bytes",
	"gpu_memory_total_bytes",
] as const;
const RAM_PROCESS_USED_BYTES_KEYS = [
	"process_ram_used_bytes",
	"ram_process_used_bytes",
	"process_memory_used_bytes",
	"memory_process_used_bytes",
	"app_ram_used_bytes",
	"app_memory_used_bytes",
	"pid_ram_used_bytes",
] as const;
const VRAM_PROCESS_USED_BYTES_KEYS = [
	"process_vram_used_bytes",
	"vram_process_used_bytes",
	"process_gpu_mem_used_bytes",
	"process_gpu_memory_used_bytes",
	"app_vram_used_bytes",
	"pid_vram_used_bytes",
] as const;

function buildCompositionChartConfig(t: TranslateFn): ChartConfig {
	return {
		process: {
			label: t("performance.composition.slice.process"),
			color: "hsl(var(--performance-color-process))",
		},
		others: {
			label: t("performance.composition.slice.others"),
			color: "hsl(var(--performance-color-others))",
		},
		freeIdle: {
			label: t("performance.composition.slice.freeIdle"),
			color: "hsl(var(--performance-color-free-idle))",
		},
	};
}

function buildUtilizationTrendChartConfig(t: TranslateFn): ChartConfig {
	return {
		cpuPercent: {
			label: t("performance.metric.cpuPercent"),
			color: "hsl(var(--performance-color-cpu))",
		},
		gpuPercent: {
			label: t("performance.metric.gpuPercent"),
			color: "hsl(var(--performance-color-gpu))",
		},
		ramPercent: {
			label: t("performance.metric.ramPercent"),
			color: "hsl(var(--performance-color-ram))",
		},
		vramPercent: {
			label: t("performance.metric.vramPercent"),
			color: "hsl(var(--performance-color-vram))",
		},
	};
}

type OverviewCard = {
	id: "cpu" | "ram" | "gpu" | "vram";
	title: string;
	primary: string;
	detail: string;
};

type CompositionSliceKey = "process" | "others" | "freeIdle";

type CompositionSlice = {
	key: CompositionSliceKey;
	label: string;
	bytes: number;
	percent: number;
	fill: string;
};

type CompositionCard = {
	id: "ram" | "vram";
	title: string;
	detail: string;
	slices: CompositionSlice[] | null;
};

type TrendMetricKey = "cpuPercent" | "gpuPercent" | "ramPercent" | "vramPercent";

type UtilizationTrendPoint = {
	rowId: string;
	timestampLabel: string;
	cpuPercent: number | null;
	gpuPercent: number | null;
	ramPercent: number | null;
	vramPercent: number | null;
};

type TimelineFallbackRow = {
	rowId: string;
	timestampLabel: string;
	cpuPercent: string;
	gpuPercent: string;
	ramPercent: string;
	vramPercent: string;
};

type TrendUnavailableReason =
	| "off"
	| "empty_series"
	| "insufficient_points"
	| "missing_metrics";

const TREND_METRIC_KEYS: readonly TrendMetricKey[] = [
	"cpuPercent",
	"gpuPercent",
	"ramPercent",
	"vramPercent",
];

type DerivableProcessPair = {
	processKey: string;
	usedKey: string;
};

type CompositionUnavailableReason =
	| "missing_used_or_total"
	| "invalid_total"
	| "missing_process"
	| "ambiguous_process";

type DeriveProcessResult =
	| { kind: "derived"; processUsedBytes: number; usedBytes: number }
	| { kind: "unavailable" }
	| { kind: "ambiguous" };

function pickMetricByKey(metrics: PerformanceMetrics | null, key: string): number | null {
	if (!metrics) {
		return null;
	}

	const metric = metrics[key];
	if (typeof metric === "number" && Number.isFinite(metric)) {
		return metric;
	}

	return null;
}

function asNonNegativeBytes(value: number | null): number | null {
	if (value === null || !Number.isFinite(value) || value < 0) {
		return null;
	}

	return value;
}

function deriveProcessBytesFromPair(
	metrics: PerformanceMetrics | null,
	pair: DerivableProcessPair,
): DeriveProcessResult {
	const processCandidate = pickMetricByKey(metrics, pair.processKey);
	const usedCandidate = pickMetricByKey(metrics, pair.usedKey);

	if (processCandidate === null || usedCandidate === null) {
		return { kind: "unavailable" };
	}

	if (processCandidate < 0 || usedCandidate < 0 || processCandidate > usedCandidate) {
		return { kind: "ambiguous" };
	}

	return {
		kind: "derived",
		processUsedBytes: processCandidate,
		usedBytes: usedCandidate,
	};
}

function compositionUnavailableDetail(
	label: string,
	status: PerformanceStatus,
	enabled: boolean,
	reason: CompositionUnavailableReason,
	t: TranslateFn,
): string {
	if (!enabled || status === "disabled") {
		return t("performance.composition.unavailable.off", { label });
	}

	const statusHint =
		status === "partial"
			? t("performance.statusHint.partial")
			: status === "degraded"
				? t("performance.statusHint.degraded")
				: "";

	if (reason === "missing_used_or_total") {
		return t("performance.composition.unavailable.missingUsedOrTotal", {
			label,
			statusHint,
		});
	}

	if (reason === "invalid_total") {
		return t("performance.composition.unavailable.invalidTotal", {
			label,
			statusHint,
		});
	}

	if (reason === "ambiguous_process") {
		return t("performance.composition.unavailable.ambiguousProcess", {
			label,
			statusHint,
		});
	}

	return t("performance.composition.unavailable.missingProcess", {
		label,
		statusHint,
	});
}

function buildCompositionCard(options: {
	id: "ram" | "vram";
	title: string;
	status: PerformanceStatus;
	enabled: boolean;
	metrics: PerformanceMetrics | null;
	usedKeys: readonly string[];
	totalKeys: readonly string[];
	processKeys: readonly string[];
	derivableProcessPair?: DerivableProcessPair;
	t: TranslateFn;
}): CompositionCard {
	const {
		id,
		title,
		status,
		enabled,
		metrics,
		usedKeys,
		totalKeys,
		processKeys,
		derivableProcessPair,
		t,
	} = options;

	const usedBytes = asNonNegativeBytes(pickMetric(metrics, usedKeys));
	const totalBytes = asNonNegativeBytes(pickMetric(metrics, totalKeys));

	if (usedBytes === null || totalBytes === null) {
		return {
			id,
			title,
			detail: compositionUnavailableDetail(
				title,
				status,
				enabled,
				"missing_used_or_total",
				t,
			),
			slices: null,
		};
	}

	if (totalBytes <= 0) {
		return {
			id,
			title,
			detail: compositionUnavailableDetail(title, status, enabled, "invalid_total", t),
			slices: null,
		};
	}

	let processUsedBytes = asNonNegativeBytes(pickMetric(metrics, processKeys));
	let resolvedUsedBytes = usedBytes;

	if (processUsedBytes === null && derivableProcessPair) {
		const derived = deriveProcessBytesFromPair(metrics, derivableProcessPair);
		if (derived.kind === "derived") {
			processUsedBytes = derived.processUsedBytes;
			resolvedUsedBytes = derived.usedBytes;
		}
		if (derived.kind === "ambiguous") {
			return {
				id,
				title,
				detail: compositionUnavailableDetail(
					title,
					status,
					enabled,
					"ambiguous_process",
					t,
				),
				slices: null,
			};
		}
	}

	if (processUsedBytes === null) {
		return {
			id,
			title,
			detail: compositionUnavailableDetail(title, status, enabled, "missing_process", t),
			slices: null,
		};
	}

	const normalizedUsedBytes = Math.min(resolvedUsedBytes, totalBytes);
	const normalizedProcessUsedBytes = Math.min(processUsedBytes, normalizedUsedBytes);
	const othersUsedBytes = Math.max(normalizedUsedBytes - normalizedProcessUsedBytes, 0);
	const freeIdleBytes = Math.max(totalBytes - normalizedUsedBytes, 0);

	const slices: CompositionSlice[] = [
		{
			key: "process",
			label: t("performance.composition.slice.process"),
			bytes: normalizedProcessUsedBytes,
			percent: (normalizedProcessUsedBytes / totalBytes) * 100,
			fill: "hsl(var(--performance-color-process))",
		},
		{
			key: "others",
			label: t("performance.composition.slice.others"),
			bytes: othersUsedBytes,
			percent: (othersUsedBytes / totalBytes) * 100,
			fill: "hsl(var(--performance-color-others))",
		},
		{
			key: "freeIdle",
			label: t("performance.composition.slice.freeIdle"),
			bytes: freeIdleBytes,
			percent: (freeIdleBytes / totalBytes) * 100,
			fill: "hsl(var(--performance-color-free-idle))",
		},
	];

	return {
		id,
		title,
		detail: t("performance.composition.usedOfTotal", {
			used: formatBytes(normalizedUsedBytes),
			total: formatBytes(totalBytes),
		}),
		slices,
	};
}

function pickMetric(metrics: PerformanceMetrics | null, keys: readonly string[]): number | null {
	if (!metrics) {
		return null;
	}

	for (const key of keys) {
		const metric = metrics[key];
		if (typeof metric === "number" && Number.isFinite(metric)) {
			return metric;
		}
	}

	return null;
}

function formatPercent(value: number | null): string {
	if (value === null || !Number.isFinite(value)) {
		return EM_DASH;
	}

	const clamped = Math.max(0, Math.min(100, value));
	return `${clamped.toFixed(1)}%`;
}

function formatBytes(value: number | null): string {
	if (value === null || !Number.isFinite(value) || value < 0) {
		return EM_DASH;
	}

	if (value === 0) {
		return "0 B";
	}

	let scaledValue = value;
	let unitIndex = 0;
	while (scaledValue >= 1024 && unitIndex < BYTE_UNITS.length - 1) {
		scaledValue /= 1024;
		unitIndex += 1;
	}

	if (unitIndex === 0) {
		return `${Math.round(scaledValue)} ${BYTE_UNITS[unitIndex]}`;
	}

	return `${scaledValue.toFixed(1)} ${BYTE_UNITS[unitIndex]}`;
}

function formatBytesPair(
	usedBytes: number | null,
	totalBytes: number | null,
	t: TranslateFn,
): string {
	if (usedBytes === null && totalBytes === null) {
		return EM_DASH;
	}

	if (usedBytes !== null && totalBytes !== null) {
		return `${formatBytes(usedBytes)} / ${formatBytes(totalBytes)}`;
	}

	if (usedBytes !== null) {
		return t("performance.value.bytesUsed", { value: formatBytes(usedBytes) });
	}

	return t("performance.value.bytesTotal", { value: formatBytes(totalBytes) });
}

function computeRatioPercent(usedBytes: number | null, totalBytes: number | null): number | null {
	if (
		usedBytes === null ||
		totalBytes === null ||
		!Number.isFinite(usedBytes) ||
		!Number.isFinite(totalBytes) ||
		totalBytes <= 0
	) {
		return null;
	}

	return (usedBytes / totalBytes) * 100;
}

function statusHintSuffix(status: PerformanceStatus, t: TranslateFn): string {
	if (status === "partial") {
		return t("performance.statusHint.partial");
	}

	if (status === "degraded") {
		return t("performance.statusHint.degraded");
	}

	return "";
}

function buildUtilizationTrendPoints(series: PerformanceSeriesPoint[]): UtilizationTrendPoint[] {
	if (series.length === 0) {
		return [];
	}

	const orderedPoints = [...series]
		.map((point, index) => ({ point, index }))
		.filter(({ point }) => Number.isFinite(point.timestamp_ms))
		.sort((left, right) => {
			if (left.point.timestamp_ms === right.point.timestamp_ms) {
				return left.index - right.index;
			}
			return left.point.timestamp_ms - right.point.timestamp_ms;
		});

	if (orderedPoints.length === 0) {
		return [];
	}

	const firstTimestampMs = orderedPoints[0].point.timestamp_ms;

	return orderedPoints.map(({ point, index }) => {
		const cpuPercent = pickMetric(point.metrics, CPU_PERCENT_KEYS);
		const gpuPercent = pickMetric(point.metrics, GPU_PERCENT_KEYS);
		const ramPercent = computeRatioPercent(
			pickMetric(point.metrics, RAM_USED_BYTES_KEYS),
			pickMetric(point.metrics, RAM_TOTAL_BYTES_KEYS),
		);
		const vramPercent = computeRatioPercent(
			pickMetric(point.metrics, VRAM_USED_BYTES_KEYS),
			pickMetric(point.metrics, VRAM_TOTAL_BYTES_KEYS),
		);
		const elapsedSeconds = Math.max(
			0,
			Math.round((point.timestamp_ms - firstTimestampMs) / 1000),
		);

		return {
			rowId: `${point.timestamp_ms}-${index}`,
			timestampLabel: `+${elapsedSeconds}s`,
			cpuPercent,
			gpuPercent,
			ramPercent,
			vramPercent,
		};
	});
}

function trendHasRenderableSamples(
	points: UtilizationTrendPoint[],
	metricKey: TrendMetricKey,
): boolean {
	let sampleCount = 0;

	for (const point of points) {
		const value = point[metricKey];
		if (typeof value === "number" && Number.isFinite(value)) {
			sampleCount += 1;
			if (sampleCount >= 2) {
				return true;
			}
		}
	}

	return false;
}

function trendUnavailableDetail(
	status: PerformanceStatus,
	enabled: boolean,
	reason: TrendUnavailableReason,
	t: TranslateFn,
): string {
	if (!enabled || status === "disabled" || reason === "off") {
		return t("performance.trend.unavailable.off");
	}

	if (reason === "empty_series") {
		return t("performance.trend.unavailable.emptySeries", {
			statusHint: statusHintSuffix(status, t),
		});
	}

	if (reason === "insufficient_points") {
		return t("performance.trend.unavailable.insufficientPoints", {
			minimum: 2,
			statusHint: statusHintSuffix(status, t),
		});
	}

	return t("performance.trend.unavailable.missingMetrics", {
		statusHint: statusHintSuffix(status, t),
	});
}

function statusBadge(status: PerformanceStatus, enabled: boolean, t: TranslateFn): string {
	if (!enabled || status === "disabled") {
		return t("performance.status.off");
	}

	if (status === "partial") {
		return t("performance.status.partial");
	}

	if (status === "degraded") {
		return t("performance.status.degraded");
	}

	return t("performance.status.live");
}

function unavailableDetail(
	label: string,
	status: PerformanceStatus,
	enabled: boolean,
	t: TranslateFn,
): string {
	if (!enabled || status === "disabled") {
		return t("performance.detail.telemetryOff", { label });
	}

	if (status === "partial") {
		return t("performance.detail.unavailablePartial", { label });
	}

	if (status === "degraded") {
		return t("performance.detail.unavailableDegraded", { label });
	}

	return t("performance.detail.unavailable", { label });
}

function overviewCardClassName(status: PerformanceStatus, enabled: boolean): string {
	if (!enabled || status === "disabled") {
		return "border-dashed border-muted-foreground/30 bg-muted/20";
	}

	return "border-border/60 bg-card";
}

export function PerformancePage() {
	const { t } = useTranslation("common");
	const [overview, setOverview] = useState<PerformanceOverviewResponse | null>(null);
	const [performanceExport, setPerformanceExport] =
		useState<PerformanceExportResponse | null>(null);
	const [fetchError, setFetchError] = useState<string | null>(null);
	const [exportFetchError, setExportFetchError] = useState<string | null>(null);

	const compositionChartConfig = useMemo(() => buildCompositionChartConfig(t), [t]);
	const utilizationTrendChartConfig = useMemo(() => buildUtilizationTrendChartConfig(t), [t]);

	const cpuLabel = t("performance.metric.cpu");
	const ramLabel = t("performance.metric.ram");
	const gpuLabel = t("performance.metric.gpu");
	const vramLabel = t("performance.metric.vram");
	const offValue = t("performance.status.off");

	useEffect(() => {
		let isCancelled = false;

		const fetchPerformanceData = async () => {
			const [overviewResult, exportResult] = await Promise.allSettled([
				getPerformanceOverview(),
				getPerformanceExport(),
			]);

			if (isCancelled) {
				return;
			}

			if (overviewResult.status === "fulfilled") {
				setOverview(overviewResult.value);
				setFetchError(null);
			} else {
				setOverview(null);
				setFetchError(
					overviewResult.reason instanceof Error
						? overviewResult.reason.message
						: t("performance.error.fetchOverview"),
				);
			}

			if (exportResult.status === "fulfilled") {
				setPerformanceExport(exportResult.value);
				setExportFetchError(null);
			} else {
				setPerformanceExport(null);
				setExportFetchError(
					exportResult.reason instanceof Error
						? exportResult.reason.message
						: t("performance.error.fetchExport"),
				);
			}
		};

		void fetchPerformanceData();
		const intervalId = window.setInterval(() => {
			void fetchPerformanceData();
		}, 5000);

		return () => {
			isCancelled = true;
			window.clearInterval(intervalId);
		};
	}, [t]);

	const status = overview?.status ?? "disabled";
	const enabled = overview?.enabled ?? false;
	const metrics = overview?.metrics ?? null;
	const summaryMessage =
		overview?.message ??
		(fetchError
			? t("performance.overview.unavailableWithError", { error: fetchError })
			: t("performance.overview.dataUnavailable"));

	const cards = useMemo<OverviewCard[]>(() => {
		const isOff = !enabled || status === "disabled";

		const cpuPercent = pickMetric(metrics, CPU_PERCENT_KEYS);
		const gpuPercent = pickMetric(metrics, GPU_PERCENT_KEYS);
		const ramUsedBytes = pickMetric(metrics, RAM_USED_BYTES_KEYS);
		const ramTotalBytes = pickMetric(metrics, RAM_TOTAL_BYTES_KEYS);
		const vramUsedBytes = pickMetric(metrics, VRAM_USED_BYTES_KEYS);
		const vramTotalBytes = pickMetric(metrics, VRAM_TOTAL_BYTES_KEYS);

		const ramUsagePercent = computeRatioPercent(ramUsedBytes, ramTotalBytes);
		const vramUsagePercent = computeRatioPercent(vramUsedBytes, vramTotalBytes);

		return [
			{
				id: "cpu",
				title: cpuLabel,
				primary: isOff ? offValue : formatPercent(cpuPercent),
				detail: isOff
					? t("performance.detail.telemetryOff", { label: cpuLabel })
					: cpuPercent !== null
						? t("performance.detail.utilization", { label: cpuLabel })
						: unavailableDetail(cpuLabel, status, enabled, t),
			},
			{
				id: "ram",
				title: ramLabel,
				primary: isOff ? offValue : formatBytesPair(ramUsedBytes, ramTotalBytes, t),
				detail: isOff
					? t("performance.detail.telemetryOff", { label: ramLabel })
					: ramUsagePercent !== null
						? t("performance.detail.usage", { value: formatPercent(ramUsagePercent) })
						: unavailableDetail(ramLabel, status, enabled, t),
			},
			{
				id: "gpu",
				title: gpuLabel,
				primary: isOff ? offValue : formatPercent(gpuPercent),
				detail: isOff
					? t("performance.detail.telemetryOff", { label: gpuLabel })
					: gpuPercent !== null
						? t("performance.detail.utilization", { label: gpuLabel })
						: unavailableDetail(gpuLabel, status, enabled, t),
			},
			{
				id: "vram",
				title: vramLabel,
				primary: isOff ? offValue : formatBytesPair(vramUsedBytes, vramTotalBytes, t),
				detail: isOff
					? t("performance.detail.telemetryOff", { label: vramLabel })
					: vramUsagePercent !== null
						? t("performance.detail.usage", {
								value: formatPercent(vramUsagePercent),
							})
						: unavailableDetail(vramLabel, status, enabled, t),
			},
		];
	}, [cpuLabel, enabled, gpuLabel, metrics, offValue, ramLabel, status, t, vramLabel]);

	const compositionCards = useMemo<CompositionCard[]>(() => {
		return [
			buildCompositionCard({
				id: "ram",
				title: ramLabel,
				status,
				enabled,
				metrics,
				usedKeys: RAM_USED_BYTES_KEYS,
				totalKeys: RAM_TOTAL_BYTES_KEYS,
				processKeys: RAM_PROCESS_USED_BYTES_KEYS,
				derivableProcessPair: {
					processKey: "memory_used_bytes",
					usedKey: "ram_used_bytes",
				},
				t,
			}),
			buildCompositionCard({
				id: "vram",
				title: vramLabel,
				status,
				enabled,
				metrics,
				usedKeys: VRAM_USED_BYTES_KEYS,
				totalKeys: VRAM_TOTAL_BYTES_KEYS,
				processKeys: VRAM_PROCESS_USED_BYTES_KEYS,
				t,
			}),
		];
	}, [enabled, metrics, ramLabel, status, t, vramLabel]);

	const trendStatus = performanceExport?.status ?? status;
	const trendEnabled = performanceExport?.enabled ?? enabled;
	const trendSummaryMessage =
		performanceExport?.message ??
		(exportFetchError
			? t("performance.trend.unavailableWithError", { error: exportFetchError })
			: t("performance.trend.dataUnavailable"));

	const utilizationTrend = useMemo(() => {
		const trendPoints = buildUtilizationTrendPoints(performanceExport?.series ?? []);
		const renderableMetricKeys = TREND_METRIC_KEYS.filter((metricKey) =>
			trendHasRenderableSamples(trendPoints, metricKey),
		);

		let unavailableReason: TrendUnavailableReason | null = null;
		if (!trendEnabled || trendStatus === "disabled") {
			unavailableReason = "off";
		} else if (trendPoints.length === 0) {
			unavailableReason = "empty_series";
		} else if (trendPoints.length < 2) {
			unavailableReason = "insufficient_points";
		} else if (renderableMetricKeys.length === 0) {
			unavailableReason = "missing_metrics";
		}

		const timelineRows: TimelineFallbackRow[] = trendPoints.map((point) => ({
			rowId: point.rowId,
			timestampLabel: point.timestampLabel,
			cpuPercent: formatPercent(point.cpuPercent),
			gpuPercent: formatPercent(point.gpuPercent),
			ramPercent: formatPercent(point.ramPercent),
			vramPercent: formatPercent(point.vramPercent),
		}));

		return {
			trendPoints,
			renderableMetricKeys,
			unavailableReason,
			timelineRows,
		};
	}, [performanceExport, trendEnabled, trendStatus]);

	return (
		<PageContainer title={t("header.nav.performance")}>
			<div className="mb-6 grid gap-4 md:grid-cols-2 xl:grid-cols-4">
				{cards.map((card) => (
					<Card
						key={card.id}
						data-testid={`performance-card-${card.id}`}
						className={overviewCardClassName(status, enabled)}
					>
						<CardHeader className="space-y-2 pb-2">
							<div className="flex items-center justify-between gap-3">
								<CardTitle className="text-sm font-medium">{card.title}</CardTitle>
								<span className="text-[10px] font-semibold tracking-[0.08em] text-muted-foreground">
									{statusBadge(status, enabled, t)}
								</span>
							</div>
							<CardDescription className="text-xs">
								{summaryMessage}
							</CardDescription>
						</CardHeader>
						<CardContent className="space-y-1">
							<p
								data-testid={`performance-value-${card.id}`}
								className="font-mono text-xl font-semibold tracking-tight"
							>
								{card.primary}
							</p>
							<p
								data-testid={`performance-detail-${card.id}`}
								className="text-xs text-muted-foreground"
							>
								{card.detail}
							</p>
						</CardContent>
					</Card>
				))}
			</div>

			<div className="mb-6 grid gap-4 xl:grid-cols-2">
				{compositionCards.map((compositionCard) => (
					<Card
						key={compositionCard.id}
						data-testid={`performance-composition-${compositionCard.id}`}
						className={overviewCardClassName(status, enabled)}
					>
						<CardHeader className="space-y-2 pb-2">
							<div className="flex items-center justify-between gap-3">
								<CardTitle className="text-sm font-medium">
									{t("performance.composition.title", { label: compositionCard.title })}
								</CardTitle>
								<span className="text-[10px] font-semibold tracking-[0.08em] text-muted-foreground">
									{statusBadge(status, enabled, t)}
								</span>
							</div>
							<CardDescription className="text-xs">{summaryMessage}</CardDescription>
						</CardHeader>
						<CardContent className="space-y-4">
							{compositionCard.slices ? (
								<>
									<ChartContainer
										data-testid={`performance-composition-chart-${compositionCard.id}`}
										ariaLabel={t("performance.composition.chartAria", {
											label: compositionCard.title,
										})}
										config={compositionChartConfig}
										className="h-60"
									>
										<PieChart>
											<ChartTooltip
												content={
													<ChartTooltipContent
														valueFormatter={(value) =>
															typeof value === "number"
																? formatBytes(value)
																: String(value ?? "")
														}
													/>
												}
											/>
											<Pie
												data={compositionCard.slices}
												dataKey="bytes"
												nameKey="label"
												innerRadius={56}
												outerRadius={88}
												paddingAngle={2}
												isAnimationActive={false}
											/>
										</PieChart>
									</ChartContainer>
									<ul
										data-testid={`performance-composition-list-${compositionCard.id}`}
										className="space-y-1"
									>
										{compositionCard.slices.map((slice) => (
										<li
											key={slice.key}
											data-testid={`performance-composition-slice-${compositionCard.id}-${slice.key}`}
											className="flex items-center justify-between gap-4 text-xs"
										>
											<span className="flex items-center gap-2 text-muted-foreground">
												<span
													data-testid={`performance-composition-marker-${compositionCard.id}-${slice.key}`}
													className="size-2 rounded-full"
													style={{ backgroundColor: slice.fill }}
												/>
												<span>{slice.label}</span>
											</span>
											<span className="font-medium">
												{formatBytes(slice.bytes)} ({formatPercent(slice.percent)})
											</span>
										</li>
										))}
									</ul>
									<p className="text-xs text-muted-foreground">{compositionCard.detail}</p>
								</>
							) : (
								<p
									data-testid={`performance-composition-unavailable-${compositionCard.id}`}
									className="text-sm text-muted-foreground"
								>
									{compositionCard.detail}
								</p>
							)}
						</CardContent>
					</Card>
				))}
			</div>

			<Card
				data-testid="performance-utilization-trends"
				className={`mb-6 ${overviewCardClassName(trendStatus, trendEnabled)}`}
			>
				<CardHeader className="space-y-2 pb-2">
					<div className="flex items-center justify-between gap-3">
						<CardTitle className="text-sm font-medium">
							{t("performance.trend.title")}
						</CardTitle>
						<span className="text-[10px] font-semibold tracking-[0.08em] text-muted-foreground">
							{statusBadge(trendStatus, trendEnabled, t)}
						</span>
					</div>
					<CardDescription className="text-xs">{trendSummaryMessage}</CardDescription>
				</CardHeader>
				<CardContent className="space-y-4">
					{utilizationTrend.unavailableReason === null ? (
						<ChartContainer
							data-testid="performance-utilization-trend-chart"
							ariaLabel={t("performance.trend.chartAria")}
							config={utilizationTrendChartConfig}
							className="h-72"
						>
							<LineChart
								data={utilizationTrend.trendPoints}
								margin={{ top: 8, right: 12, left: 4, bottom: 0 }}
							>
								<CartesianGrid vertical={false} strokeDasharray="4 4" />
								<XAxis
									axisLine={false}
									dataKey="timestampLabel"
									tickLine={false}
								/>
								<YAxis
									axisLine={false}
									tickLine={false}
									width={40}
									domain={[0, 100]}
									tickFormatter={(value) => `${value}%`}
								/>
								<ChartTooltip
									content={
										<ChartTooltipContent
											valueFormatter={(value) =>
												typeof value === "number"
													? `${value.toFixed(1)}%`
													: String(value ?? "")
											}
										/>
									}
								/>
								{utilizationTrend.renderableMetricKeys.map((metricKey) => (
									<Line
										key={metricKey}
										dataKey={metricKey}
										type="monotone"
										stroke={`var(--color-${metricKey})`}
										strokeWidth={2}
										dot={false}
										isAnimationActive={false}
									/>
								))}
							</LineChart>
						</ChartContainer>
					) : (
						<>
							<p
								data-testid="performance-utilization-trend-unavailable"
								className="text-sm text-muted-foreground"
							>
								{trendUnavailableDetail(
									trendStatus,
									trendEnabled,
									utilizationTrend.unavailableReason,
									t,
								)}
							</p>
							<div
								data-testid="performance-utilization-timeline-fallback"
								className="overflow-x-auto rounded-md border border-border/60"
							>
								<table className="w-full min-w-[560px] table-fixed border-collapse text-xs">
									<thead>
										<tr className="border-b border-border/60 text-left text-muted-foreground">
											<th className="px-3 py-2 font-medium">
												{t("performance.trend.timeline.header")}
											</th>
											<th className="px-3 py-2 font-medium">
												{t("performance.metric.cpuPercent")}
											</th>
											<th className="px-3 py-2 font-medium">
												{t("performance.metric.gpuPercent")}
											</th>
											<th className="px-3 py-2 font-medium">
												{t("performance.metric.ramPercent")}
											</th>
											<th className="px-3 py-2 font-medium">
												{t("performance.metric.vramPercent")}
											</th>
										</tr>
									</thead>
									<tbody>
										{utilizationTrend.timelineRows.length > 0 ? (
											utilizationTrend.timelineRows.map((row) => (
												<tr
													key={row.rowId}
													data-testid={`performance-utilization-timeline-row-${row.rowId}`}
													className="border-b border-border/40 last:border-b-0"
												>
													<td className="px-3 py-2 font-mono text-muted-foreground">
														{row.timestampLabel}
													</td>
													<td className="px-3 py-2 font-mono">{row.cpuPercent}</td>
													<td className="px-3 py-2 font-mono">{row.gpuPercent}</td>
													<td className="px-3 py-2 font-mono">{row.ramPercent}</td>
													<td className="px-3 py-2 font-mono">{row.vramPercent}</td>
												</tr>
											))
										) : (
											<tr>
												<td
													colSpan={5}
													className="px-3 py-4 text-sm text-muted-foreground"
												>
													{t("performance.trend.timeline.empty")}
												</td>
											</tr>
										)}
									</tbody>
								</table>
							</div>
						</>
					)}
				</CardContent>
			</Card>

		</PageContainer>
	);
}
