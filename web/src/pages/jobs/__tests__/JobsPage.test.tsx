import {
	act,
	fireEvent,
	render,
	screen,
	waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { i18n, initializeI18n } from "@/i18n";
import { useJobStore } from "@/stores/job-store";
import type { Job } from "@/types";
import { JobsPage } from "../JobsPage";
import { formatDuration, formatETA } from "../time-utils";

vi.mock("../RunWorkflowDialog", () => ({
	RunWorkflowDialog: () => null,
}));

function makeJob(overrides: Partial<Job> = {}): Job {
	return {
		id: "job-1",
		status: "completed",
		progress: null,
		created_at: "2025-01-01T00:00:00Z",
		started_at: null,
		completed_at: null,
		error: null,
		workflow_name: "Manual Workflow",
		workflow_source: "api_jobs",
		params: null,
		...overrides,
	};
}

beforeEach(async () => {
	initializeI18n();
	await i18n.changeLanguage("en");

	vi.clearAllMocks();
	useJobStore.setState({
		jobs: [],
		activeJobId: null,
		activeProgress: null,
		runtimePreviewsByNodeId: {},
		runtimePreviewsByJobId: {},
		wsCleanup: null,
		fetchJobs: vi.fn().mockResolvedValue(undefined),
		submitJob: vi.fn().mockResolvedValue("job-1"),
		cancelJob: vi.fn().mockResolvedValue(undefined),
		rerunJob: vi.fn().mockResolvedValue("job-rerun"),
		deleteJobHistory: vi.fn().mockResolvedValue(undefined),
		subscribeToJob: vi.fn(),
		unsubscribeFromJob: vi.fn(),
	});
});

afterEach(() => {
	vi.useRealTimers();
});

function durationForSeconds(totalSeconds: number): string {
	const start = new Date(0).toISOString();
	const end = new Date(totalSeconds * 1000).toISOString();
	return formatDuration(start, end);
}

describe("jobs time formatting", () => {
	describe("formatDuration", () => {
		it("formats hh:mm:ss for common durations", () => {
			expect(durationForSeconds(6595)).toBe("01:49:55");
			expect(durationForSeconds(45)).toBe("00:00:45");
			expect(durationForSeconds(3600)).toBe("01:00:00");
		});

		it("formats edge durations", () => {
			expect(durationForSeconds(0)).toBe("00:00:00");
			expect(durationForSeconds(86400)).toBe("24:00:00");
		});
	});

	describe("formatETA", () => {
		it("formats positive values as hh:mm:ss remaining", () => {
			expect(formatETA(332)).toBe("~00:05:32 remaining");
			expect(formatETA(3661)).toBe("~01:01:01 remaining");
		});

		it("returns em dash for null and non-positive values", () => {
			expect(formatETA(null)).toBe("—");
			expect(formatETA(0)).toBe("—");
			expect(formatETA(-1)).toBe("—");
		});
	});
});

describe("JobsPage lifecycle", () => {
	it("polls jobs every 2 seconds as fallback", async () => {
		vi.useFakeTimers();

		const fetchJobs = vi.fn().mockResolvedValue(undefined);
		useJobStore.setState({ fetchJobs });

		const { unmount } = render(<JobsPage />);

		await act(async () => {
			await Promise.resolve();
		});
		expect(fetchJobs).toHaveBeenCalledTimes(1);

		await act(async () => {
			vi.advanceTimersByTime(2000);
		});
		expect(fetchJobs).toHaveBeenCalledTimes(2);

		await act(async () => {
			vi.advanceTimersByTime(2000);
		});
		expect(fetchJobs).toHaveBeenCalledTimes(3);

		unmount();
	});

	it("subscribes to active jobs, switches job streams, and unsubscribes on completion/unmount", async () => {
		const fetchJobs = vi.fn().mockResolvedValue(undefined);
		const subscribeToJob = vi.fn((jobId: string) => {
			useJobStore.setState({ activeJobId: jobId });
		});
		const unsubscribeFromJob = vi.fn(() => {
			useJobStore.setState({
				activeJobId: null,
				activeProgress: null,
				wsCleanup: null,
			});
		});

		useJobStore.setState({
			jobs: [makeJob({ id: "job-1", status: "running" })],
			activeJobId: null,
			fetchJobs,
			subscribeToJob,
			unsubscribeFromJob,
		});

		const { unmount } = render(<JobsPage />);

		await waitFor(() => {
			expect(subscribeToJob).toHaveBeenCalledWith("job-1");
		});

		act(() => {
			useJobStore.setState({
				jobs: [
					makeJob({ id: "job-1", status: "completed" }),
					makeJob({ id: "job-2", status: "running" }),
				],
			});
		});

		await waitFor(() => {
			expect(subscribeToJob).toHaveBeenCalledWith("job-2");
		});

		act(() => {
			useJobStore.setState({
				jobs: [
					makeJob({ id: "job-1", status: "completed" }),
					makeJob({ id: "job-2", status: "completed" }),
				],
				activeJobId: "job-2",
			});
		});

		await waitFor(() => {
			expect(unsubscribeFromJob).toHaveBeenCalledTimes(1);
		});

		unmount();
		expect(unsubscribeFromJob).toHaveBeenCalledTimes(2);
	});

	it("keeps input FPS and ETA visible even before first progress event", async () => {
		useJobStore.setState({
			jobs: [makeJob({ id: "job-active", status: "running", progress: null })],
			activeJobId: "job-active",
			activeProgress: null,
		});

		render(<JobsPage />);

		await waitFor(() => {
			expect(screen.getByTestId("jobs-active-stat-input-fps")).toHaveTextContent(
				"0.0",
			);
			expect(screen.getByTestId("jobs-active-stat-eta")).toHaveTextContent(
				"00:00:00",
			);
		});
	});
});

describe("JobsPage print debug visibility", () => {
	it("shows Print output in matching run details when debug events exist", async () => {
		useJobStore.setState({
			jobs: [makeJob({ id: "print-job-1", status: "completed" })],
			runtimePreviewsByJobId: {
				"print-job-1": {
					"node-print-1": {
						node_id: "node-print-1",
						node_type: "Print",
						value_preview: "print-debug-value",
						truncated: false,
						preview_max_chars: 512,
						updated_at_ms: 123,
					},
				},
			},
		});

		render(<JobsPage />);

		const rowButton = screen.getByText("print-jo").closest("button");
		expect(rowButton).not.toBeNull();
		fireEvent.click(rowButton as HTMLButtonElement);

		await waitFor(() => {
			expect(screen.getByText("Print output")).toBeInTheDocument();
			expect(screen.getByText("node-print-1")).toBeInTheDocument();
			expect(screen.getByText("print-debug-value")).toBeInTheDocument();
		});
	});

	it("does not render Print output section when run has no debug events", async () => {
		useJobStore.setState({
			jobs: [makeJob({ id: "plain-job-1", status: "completed" })],
			runtimePreviewsByJobId: {},
		});

		render(<JobsPage />);

		const rowButton = screen.getByText("plain-jo").closest("button");
		expect(rowButton).not.toBeNull();
		fireEvent.click(rowButton as HTMLButtonElement);

		await waitFor(() => {
			expect(screen.queryByText("Print output")).not.toBeInTheDocument();
		});
	});
});

describe("JobsPage history actions", () => {
	it("shows delete for completed rows and hides retry", () => {
		useJobStore.setState({
			jobs: [makeJob({ id: "completed-job", status: "completed" })],
		});

		render(<JobsPage />);

		expect(screen.getByRole("button", { name: "Delete" })).toBeInTheDocument();
		expect(screen.queryByRole("button", { name: "Retry" })).not.toBeInTheDocument();
	});

	it("shows retry for non-completed rows", () => {
		useJobStore.setState({
			jobs: [makeJob({ id: "failed-job", status: "failed" })],
		});

		render(<JobsPage />);

		expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
		expect(screen.getByRole("button", { name: "Delete" })).toBeInTheDocument();
	});

	it("calls rerunJob when retry button is clicked", async () => {
		const rerunJob = vi.fn().mockResolvedValue("job-rerun");
		useJobStore.setState({
			jobs: [makeJob({ id: "retryable-job", status: "failed" })],
			rerunJob,
		});

		render(<JobsPage />);

		fireEvent.click(screen.getByRole("button", { name: "Retry" }));

		await waitFor(() => {
			expect(rerunJob).toHaveBeenCalledWith("retryable-job");
		});
	});

	it("calls deleteJobHistory when delete button is clicked", async () => {
		const deleteJobHistory = vi.fn().mockResolvedValue(undefined);
		useJobStore.setState({
			jobs: [makeJob({ id: "deletable-job", status: "completed" })],
			deleteJobHistory,
		});

		render(<JobsPage />);

		fireEvent.click(screen.getByRole("button", { name: "Delete" }));

		await waitFor(() => {
			expect(deleteJobHistory).toHaveBeenCalledWith("deletable-job");
		});
	});

	it("shows workflow source and full params JSON in expanded details", async () => {
		useJobStore.setState({
			jobs: [
				makeJob({
					id: "detail-job-1",
					status: "failed",
					workflow_name: "Named Workflow",
					workflow_source: "api_run_workflows",
					params: { quality: "high", retries: 2 },
				}),
			],
		});

		render(<JobsPage />);

		const rowButton = screen.getByText("detail-j").closest("button");
		expect(rowButton).not.toBeNull();
		fireEvent.click(rowButton as HTMLButtonElement);

		await waitFor(() => {
			expect(screen.getByText("api_run_workflows")).toBeInTheDocument();
			expect(screen.getByText((content) => content.includes('"quality": "high"'))).toBeInTheDocument();
			expect(screen.getByText((content) => content.includes('"retries": 2'))).toBeInTheDocument();
		});
	});

	it("renders safe params summary for null params", () => {
		useJobStore.setState({
			jobs: [makeJob({ id: "null-params-job", status: "completed", params: null })],
		});

		render(<JobsPage />);

		expect(screen.getByText("No params")).toBeInTheDocument();
	});
});
