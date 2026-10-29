import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ApiError } from "@/api/client";
import { toast } from "@/components/shared/Toaster";
import { i18n, initializeI18n } from "@/i18n";
import { useJobStore } from "@/stores/job-store";
import type { Job } from "@/types";
import { JobsPage } from "../JobsPage";

vi.mock("../RunWorkflowDialog", () => ({
	RunWorkflowDialog: () => null,
}));

vi.mock("@/components/shared/Toaster", () => ({
	toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() },
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
		refreshError: null,
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

describe("JobsPage mutation failure feedback", () => {
	it("shows the server message when cancelling the active job fails", async () => {
		const cancelJob = vi.fn().mockRejectedValue(new ApiError(500, "cancel failed"));
		useJobStore.setState({
			jobs: [makeJob({ id: "running-job", status: "running" })],
			activeJobId: "running-job",
			cancelJob,
		});

		render(<JobsPage />);
		const cancelButton = screen.getByRole("button", { name: "Cancel" });
		fireEvent.click(cancelButton);

		await waitFor(() => {
			expect(cancelJob).toHaveBeenCalledWith("running-job");
			expect(cancelButton).not.toBeDisabled();
		});
		expect(toast.error).toHaveBeenCalledWith("Failed to cancel job: cancel failed");
	});

	it("shows the server message when retrying a job fails", async () => {
		const rerunJob = vi.fn().mockRejectedValue(new ApiError(500, "rerun failed"));
		useJobStore.setState({
			jobs: [makeJob({ id: "failed-job", status: "failed" })],
			rerunJob,
		});

		render(<JobsPage />);
		const retryButton = screen.getByRole("button", { name: "Retry" });
		fireEvent.click(retryButton);

		await waitFor(() => {
			expect(rerunJob).toHaveBeenCalledWith("failed-job");
			expect(retryButton).not.toBeDisabled();
		});
		expect(toast.error).toHaveBeenCalledWith("Failed to retry job: rerun failed");
	});

	it("shows the server message when deleting job history fails", async () => {
		const deleteJobHistory = vi.fn().mockRejectedValue(new ApiError(500, "delete failed"));
		useJobStore.setState({
			jobs: [makeJob({ id: "done-job", status: "completed" })],
			deleteJobHistory,
		});

		render(<JobsPage />);
		const deleteButton = screen.getByRole("button", { name: "Delete" });
		fireEvent.click(deleteButton);

		await waitFor(() => {
			expect(deleteJobHistory).toHaveBeenCalledWith("done-job");
			expect(deleteButton).not.toBeDisabled();
		});
		expect(toast.error).toHaveBeenCalledWith(
			"Failed to delete job history: delete failed",
		);
	});

	it("leaves an expired session to the sign-in screen instead of toasting", async () => {
		const deleteJobHistory = vi.fn().mockRejectedValue(new ApiError(401, "unauthorized"));
		useJobStore.setState({
			jobs: [makeJob({ id: "done-job", status: "completed" })],
			deleteJobHistory,
		});

		render(<JobsPage />);
		const deleteButton = screen.getByRole("button", { name: "Delete" });
		fireEvent.click(deleteButton);

		await waitFor(() => {
			expect(deleteJobHistory).toHaveBeenCalledWith("done-job");
			expect(deleteButton).not.toBeDisabled();
		});
		expect(toast.error).not.toHaveBeenCalled();
	});
});
