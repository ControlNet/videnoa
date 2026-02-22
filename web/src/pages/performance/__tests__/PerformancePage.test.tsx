import { render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { getPerformanceExport, getPerformanceOverview } from "@/api/client";
import { i18n, initializeI18n } from "@/i18n";
import type {
	PerformanceExportResponse,
	PerformanceOverviewResponse,
	PerformanceSeriesPoint,
} from "@/types";
import { PerformancePage } from "../PerformancePage";

vi.mock("recharts", () => ({
	CartesianGrid: () => null,
	Line: () => null,
	LineChart: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
	Pie: () => null,
	PieChart: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
	ResponsiveContainer: ({ children }: { children?: ReactNode }) => (
		<div>{children}</div>
	),
	Tooltip: () => null,
	XAxis: () => null,
	YAxis: () => null,
}));

vi.mock("@/api/client", () => ({
	getPerformanceOverview: vi.fn(),
	getPerformanceExport: vi.fn(),
}));

function makeOverviewPayload(
	overrides: Partial<PerformanceOverviewResponse> = {},
): PerformanceOverviewResponse {
	return {
		status: "enabled",
		enabled: true,
		reason: "collector_ok",
		message: "telemetry available",
		metrics: {
			cpu_util_percent: 31.25,
			ram_used_bytes: 8 * 1024 * 1024 * 1024,
			ram_total_bytes: 16 * 1024 * 1024 * 1024,
			process_ram_used_bytes: 2 * 1024 * 1024 * 1024,
			gpu_util_percent: 55.5,
			vram_used_bytes: 3 * 1024 * 1024 * 1024,
			vram_total_bytes: 8 * 1024 * 1024 * 1024,
			process_vram_used_bytes: 1 * 1024 * 1024 * 1024,
		},
		...overrides,
	};
}

function makeExportPayload(
	overrides: Partial<PerformanceExportResponse> = {},
): PerformanceExportResponse {
	return {
		status: "enabled",
		enabled: true,
		reason: "collector_ok",
		message: "telemetry export ready",
		series: [
			{
				timestamp_ms: 1700000000000,
				metrics: {
					cpu_util_percent: 20,
					gpu_util_percent: 42,
					ram_used_bytes: 4 * 1024 * 1024 * 1024,
					ram_total_bytes: 8 * 1024 * 1024 * 1024,
					vram_used_bytes: 2 * 1024 * 1024 * 1024,
					vram_total_bytes: 8 * 1024 * 1024 * 1024,
				},
			},
			{
				timestamp_ms: 1700000001000,
				metrics: {
					cpu_util_percent: 30,
					gpu_util_percent: 50,
					ram_used_bytes: 5 * 1024 * 1024 * 1024,
					ram_total_bytes: 8 * 1024 * 1024 * 1024,
					vram_used_bytes: 3 * 1024 * 1024 * 1024,
					vram_total_bytes: 8 * 1024 * 1024 * 1024,
				},
			},
		],
		...overrides,
	};
}

function makeDenseSeries(count: number = 900): PerformanceSeriesPoint[] {
	const baseTimestamp = 1700000000000;
	return Array.from({ length: count }, (_, index) => ({
		timestamp_ms: baseTimestamp + index * 1000,
		metrics: {
			cpu_util_percent: 10 + ((index % 50) * 0.3),
			gpu_util_percent: 20 + ((index % 60) * 0.5),
			ram_used_bytes: 4 * 1024 * 1024 * 1024 + index * 1024,
			ram_total_bytes: 12 * 1024 * 1024 * 1024,
			vram_used_bytes: 2 * 1024 * 1024 * 1024 + index * 512,
			vram_total_bytes: 6 * 1024 * 1024 * 1024,
		},
	}));
}

beforeEach(async () => {
	vi.clearAllMocks();
	initializeI18n();
	await i18n.changeLanguage("en");
});

describe("PerformancePage overview cards", () => {
	it("renders CPU/RAM/GPU/VRAM summary cards from overview metrics", async () => {
		vi.mocked(getPerformanceOverview).mockResolvedValue(makeOverviewPayload());
		vi.mocked(getPerformanceExport).mockResolvedValue(makeExportPayload());

		render(<PerformancePage />);

		await waitFor(() => {
			expect(getPerformanceOverview).toHaveBeenCalledTimes(1);
			expect(getPerformanceExport).toHaveBeenCalledTimes(1);
		});

		expect(screen.getByTestId("performance-value-cpu")).toHaveTextContent("31.3%");
		expect(screen.getByTestId("performance-value-ram")).toHaveTextContent(
			"8.0 GiB / 16.0 GiB",
		);
		expect(screen.getByTestId("performance-value-gpu")).toHaveTextContent("55.5%");
		expect(screen.getByTestId("performance-value-vram")).toHaveTextContent(
			"3.0 GiB / 8.0 GiB",
		);
		expect(
			screen.getByTestId("performance-composition-slice-ram-process"),
		).toHaveTextContent("process");
		expect(
			screen.getByTestId("performance-composition-slice-ram-process"),
		).toHaveTextContent("2.0 GiB (12.5%)");
		expect(
			screen.getByTestId("performance-composition-slice-ram-others"),
		).toHaveTextContent("6.0 GiB (37.5%)");
		expect(
			screen.getByTestId("performance-composition-slice-ram-freeIdle"),
		).toHaveTextContent("8.0 GiB (50.0%)");
		expect(
			screen.getByTestId("performance-composition-slice-vram-process"),
		).toHaveTextContent("1.0 GiB (12.5%)");
		expect(
			screen.getByTestId("performance-composition-marker-ram-process"),
		).toHaveStyle({ backgroundColor: "hsl(var(--performance-color-process))" });
		expect(
			screen.getByTestId("performance-composition-marker-ram-others"),
		).toHaveStyle({ backgroundColor: "hsl(var(--performance-color-others))" });
		expect(
			screen.getByTestId("performance-composition-marker-ram-freeIdle"),
		).toHaveStyle({ backgroundColor: "hsl(var(--performance-color-free-idle))" });
		expect(screen.getByTestId("performance-utilization-trend-chart")).toBeInTheDocument();
	});

	it("renders utilization trend chart for dense 900-point export series", async () => {
		vi.mocked(getPerformanceOverview).mockResolvedValue(makeOverviewPayload());
		vi.mocked(getPerformanceExport).mockResolvedValue(
			makeExportPayload({
				series: makeDenseSeries(900),
			}),
		);

		render(<PerformancePage />);

		await waitFor(() => {
			expect(getPerformanceOverview).toHaveBeenCalledTimes(1);
			expect(getPerformanceExport).toHaveBeenCalledTimes(1);
		});

		expect(screen.getByTestId("performance-utilization-trend-chart")).toBeInTheDocument();
		expect(screen.queryByTestId("performance-utilization-trend-unavailable")).not.toBeInTheDocument();
		expect(screen.queryByTestId("performance-utilization-timeline-fallback")).not.toBeInTheDocument();
	});

	it("shows deterministic OFF fallback when overview status is disabled", async () => {
		vi.mocked(getPerformanceOverview).mockResolvedValue(
			makeOverviewPayload({
				status: "disabled",
				enabled: false,
				reason: "disabled_by_config",
				message: "telemetry disabled",
				metrics: null,
			}),
		);
		vi.mocked(getPerformanceExport).mockResolvedValue(
			makeExportPayload({
				status: "disabled",
				enabled: false,
				reason: "disabled_by_config",
				message: "telemetry disabled",
				series: [],
			}),
		);

		render(<PerformancePage />);

		await waitFor(() => {
			expect(screen.getByTestId("performance-value-cpu")).toHaveTextContent(
				"OFF",
			);
		});

		expect(screen.getByTestId("performance-value-ram")).toHaveTextContent("OFF");
		expect(screen.getByTestId("performance-value-gpu")).toHaveTextContent("OFF");
		expect(screen.getByTestId("performance-value-vram")).toHaveTextContent("OFF");
		expect(screen.getByTestId("performance-utilization-trend-unavailable")).toHaveTextContent(
			"Utilization trends unavailable: telemetry is off",
		);
		expect(
			screen.getByTestId("performance-utilization-timeline-fallback"),
		).toBeInTheDocument();
		expect(
			screen.getByTestId("performance-composition-unavailable-ram"),
		).toHaveTextContent("RAM composition unavailable: telemetry is off");
	});

	it("uses partial fallback text when telemetry is only partially available", async () => {
		vi.mocked(getPerformanceOverview).mockResolvedValue(
			makeOverviewPayload({
				status: "partial",
				enabled: true,
				reason: "gpu_missing",
				message: "gpu telemetry is partial",
				metrics: {
					cpu_util_percent: 24.1,
					ram_used_bytes: 4 * 1024 * 1024 * 1024,
				},
			}),
		);
		vi.mocked(getPerformanceExport).mockResolvedValue(
			makeExportPayload({
				status: "partial",
				enabled: true,
				reason: "cpu_only",
				message: "partial utilization export",
				series: [
					{
						timestamp_ms: 1700000000000,
						metrics: {
							cpu_util_percent: 21,
						},
					},
				],
			}),
		);

		render(<PerformancePage />);

		await waitFor(() => {
			expect(screen.getByTestId("performance-value-gpu")).toHaveTextContent("—");
		});

		const cpuCard = screen.getByTestId("performance-card-cpu");
		expect(cpuCard.className).toContain("border-border/60");
		expect(cpuCard.className).toContain("bg-card");
		expect(cpuCard.className).not.toContain("amber");
		expect(cpuCard.className).not.toContain("orange");

		expect(screen.getByTestId("performance-detail-gpu")).toHaveTextContent(
			"GPU unavailable (partial telemetry)",
		);
		expect(screen.getByTestId("performance-detail-vram")).toHaveTextContent(
			"VRAM unavailable (partial telemetry)",
		);
		expect(
			screen.getByTestId("performance-composition-unavailable-ram"),
		).toHaveTextContent("RAM composition unavailable: used/total bytes are missing");
		expect(screen.getByTestId("performance-utilization-trend-unavailable")).toHaveTextContent(
			"Utilization trends unavailable: need at least 2 timeline samples (partial telemetry)",
		);
	});

	it("falls back deterministically when derived RAM process bytes are ambiguous", async () => {
		vi.mocked(getPerformanceOverview).mockResolvedValue(
			makeOverviewPayload({
				metrics: {
					cpu_util_percent: 12,
					ram_used_bytes: 4 * 1024 * 1024 * 1024,
					ram_total_bytes: 16 * 1024 * 1024 * 1024,
					memory_used_bytes: 6 * 1024 * 1024 * 1024,
				},
			}),
		);
		vi.mocked(getPerformanceExport).mockResolvedValue(makeExportPayload());

		render(<PerformancePage />);

		await waitFor(() => {
			expect(
				screen.getByTestId("performance-composition-unavailable-ram"),
			).toHaveTextContent(
				"RAM composition unavailable: process-used bytes are ambiguous",
			);
		});
	});

	it("renders timeline fallback table when utilization keys are missing", async () => {
		vi.mocked(getPerformanceOverview).mockResolvedValue(
			makeOverviewPayload({
				status: "degraded",
				enabled: true,
				reason: "sampler_stale",
				message: "degraded telemetry",
				metrics: {
					cpu_util_percent: 20,
					ram_used_bytes: 5 * 1024 * 1024 * 1024,
					ram_total_bytes: 8 * 1024 * 1024 * 1024,
				},
			}),
		);
		vi.mocked(getPerformanceExport).mockResolvedValue(
			makeExportPayload({
				status: "degraded",
				enabled: true,
				reason: "export_missing_keys",
				message: "degraded export",
				series: [
					{
						timestamp_ms: 1700000000000,
						metrics: {
							temperature_celsius: 60,
						},
					},
					{
						timestamp_ms: 1700000001000,
						metrics: {
							temperature_celsius: 61,
						},
					},
				],
			}),
		);

		render(<PerformancePage />);

		await waitFor(() => {
			expect(screen.getByTestId("performance-detail-gpu")).toHaveTextContent(
				"GPU unavailable (degraded telemetry)",
			);
			expect(screen.getByTestId("performance-detail-vram")).toHaveTextContent(
				"VRAM unavailable (degraded telemetry)",
			);
			expect(
				screen.getByTestId("performance-utilization-trend-unavailable"),
			).toHaveTextContent(
				"Utilization trends unavailable: utilization keys are missing from series (degraded telemetry)",
			);
		});

		expect(
			screen.getByTestId("performance-utilization-timeline-fallback"),
		).toBeInTheDocument();
		expect(screen.getByText("Timeline")).toBeInTheDocument();
		expect(screen.getByText("CPU %")).toBeInTheDocument();
		expect(screen.getByText("GPU %")).toBeInTheDocument();
		expect(screen.getByText("RAM %")).toBeInTheDocument();
		expect(screen.getByText("VRAM %")).toBeInTheDocument();
		expect(
			screen.getByTestId("performance-utilization-timeline-row-1700000000000-0"),
		).toHaveTextContent("+0s");
		expect(
			screen.getByTestId("performance-utilization-timeline-row-1700000001000-1"),
		).toHaveTextContent("+1s");
	});
});
