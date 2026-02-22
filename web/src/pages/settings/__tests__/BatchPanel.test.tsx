import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { submitJob, submitJobWithParams } from "@/api/client";
import { i18n, initializeI18n } from "@/i18n";
import { useUIStore } from "@/stores/ui-store";
import { useWorkflowStore } from "@/stores/workflow-store";
import type { CreateJobResponse, Workflow, WorkflowPort } from "@/types";
import { BatchPanel } from "../BatchPanel";

const workflowStoreHarness = vi.hoisted(() => ({
	nodes: [] as Array<{ id: string }>,
	workflow: {
		nodes: [],
		connections: [],
		interface: { inputs: [], outputs: [] },
	} as Workflow,
}));

vi.mock("@/api/client", () => ({
	submitJob: vi.fn(),
	submitJobWithParams: vi.fn(),
}));

vi.mock("@/stores/workflow-store", () => {
	const getState = () => ({
		nodes: workflowStoreHarness.nodes,
		exportWorkflow: () => workflowStoreHarness.workflow,
	});

	const useWorkflowStoreMock = (<T,>(
		selector: (state: ReturnType<typeof getState>) => T,
	) => selector(getState())) as unknown as typeof useWorkflowStore;

	(
		useWorkflowStoreMock as unknown as {
			getState: typeof getState;
		}
	).getState = getState;

	return { useWorkflowStore: useWorkflowStoreMock };
});

vi.mock("@/components/shared/Toaster", () => ({
	toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() },
}));

function makeJobResponse(id: string): CreateJobResponse {
	return {
		id,
		status: "queued",
		created_at: new Date(0).toISOString(),
	};
}

function setWorkflowInputs(inputs: WorkflowPort[]) {
	workflowStoreHarness.nodes = [{ id: "workflow-input" }];
	workflowStoreHarness.workflow = {
		nodes: [
			{
				id: "workflow-input",
				node_type: "WorkflowInput",
				params: {
					ports: inputs,
				},
			},
		],
		connections: [],
		interface: {
			inputs,
			outputs: [],
		},
	};
}

function renderBatchPanel(inputs: WorkflowPort[]) {
	setWorkflowInputs(inputs);
	useUIStore.setState({ activeModal: "batch" });

	render(
		<MemoryRouter>
			<BatchPanel />
		</MemoryRouter>,
	);
}

beforeEach(async () => {
	initializeI18n();
	await i18n.changeLanguage("en");

	vi.clearAllMocks();
	useUIStore.setState({ activeModal: null });
	setWorkflowInputs([]);
});

describe("BatchPanel", () => {
	it("uses workflow_inputs mode when workflow exposes interface inputs", () => {
		renderBatchPanel([{ name: "video_path", port_type: "Path" }]);

		expect(screen.getByText("Row-wise by line")).toBeInTheDocument();
		expect(
			screen.getByLabelText("video_path values (one per line)"),
		).toBeInTheDocument();
		expect(screen.queryByLabelText("Repeat count")).not.toBeInTheDocument();
	});

	it("falls back to repeat_count mode when workflow has zero inputs", () => {
		renderBatchPanel([]);

		expect(screen.getByText("Template row with repeat count")).toBeInTheDocument();
		expect(screen.getByLabelText("Repeat count")).toBeInTheDocument();
		expect(screen.queryByText(/values \(one per line\)/)).not.toBeInTheDocument();
	});

	it("blocks submit on line mismatch and makes zero API calls", async () => {
		renderBatchPanel([
			{ name: "alpha", port_type: "Str" },
			{ name: "beta", port_type: "Str" },
		]);

		fireEvent.change(screen.getByLabelText("alpha values (one per line)"), {
			target: { value: "a1\na2" },
		});
		fireEvent.change(screen.getByLabelText("beta values (one per line)"), {
			target: { value: "b1" },
		});

		fireEvent.click(screen.getByRole("button", { name: "Submit Batch" }));

		await waitFor(() => {
			expect(
				screen.getByText(/1 row has mismatched line counts across ports\./),
			).toBeInTheDocument();
		});
		expect(
			screen.getByText(/beta row 2: line count mismatch \(expected 2, got 1\)/),
		).toBeInTheDocument();
		expect(
			screen.getByText(
				/Fail-fast is enabled, submission stops at the first invalid row\./,
			),
		).toBeInTheDocument();

		expect(submitJob).not.toHaveBeenCalled();
		expect(submitJobWithParams).not.toHaveBeenCalled();
	});

	it("blocks required-empty rows with port + row diagnostics and zero API calls", async () => {
		renderBatchPanel([
			{ name: "source", port_type: "Path" },
			{ name: "count", port_type: "Int" },
		]);

		fireEvent.change(screen.getByLabelText("source values (one per line)"), {
			target: { value: "/tmp/input.mkv" },
		});
		fireEvent.change(screen.getByLabelText("count values (one per line)"), {
			target: { value: " " },
		});

		fireEvent.click(screen.getByRole("button", { name: "Submit Batch" }));

		await waitFor(() => {
			expect(screen.getByText(/1 required value is missing\./)).toBeInTheDocument();
		});
		expect(
			screen.getByText(/count row 1: required value is blank/),
		).toBeInTheDocument();
		expect(
			screen.getByText(
				/Fail-fast is enabled, submission stops at the first invalid row\./,
			),
		).toBeInTheDocument();

		expect(submitJob).not.toHaveBeenCalled();
		expect(submitJobWithParams).not.toHaveBeenCalled();
	});

	it("submits row-wise params in deterministic row order", async () => {
		renderBatchPanel([
			{ name: "title", port_type: "Str" },
			{ name: "episode", port_type: "Int" },
			{ name: "dry_run", port_type: "Bool", default_value: false },
		]);

		vi.mocked(submitJobWithParams)
			.mockResolvedValueOnce(makeJobResponse("job-1"))
			.mockResolvedValueOnce(makeJobResponse("job-2"))
			.mockResolvedValueOnce(makeJobResponse("job-3"));

		fireEvent.change(screen.getByLabelText("title values (one per line)"), {
			target: { value: "alpha\nbeta\ngamma" },
		});
		fireEvent.change(screen.getByLabelText("episode values (one per line)"), {
			target: { value: "1\n2\n3" },
		});
		fireEvent.change(screen.getByLabelText("dry_run values (one per line)"), {
			target: { value: "true\n\nfalse" },
		});

		fireEvent.click(screen.getByRole("button", { name: "Submit Batch" }));

		const workflow = useWorkflowStore.getState().exportWorkflow();

		await waitFor(() => {
			expect(submitJobWithParams).toHaveBeenCalledTimes(3);
		});
		expect(submitJob).not.toHaveBeenCalled();

		expect(submitJobWithParams).toHaveBeenNthCalledWith(1, workflow, {
			title: "alpha",
			episode: 1,
			dry_run: true,
		});
		expect(submitJobWithParams).toHaveBeenNthCalledWith(2, workflow, {
			title: "beta",
			episode: 2,
			dry_run: false,
		});
		expect(submitJobWithParams).toHaveBeenNthCalledWith(3, workflow, {
			title: "gamma",
			episode: 3,
			dry_run: false,
		});
	});

	it("fails fast when the second row submission errors and does not send third row", async () => {
		renderBatchPanel([{ name: "video_path", port_type: "Path" }]);

		vi.mocked(submitJobWithParams)
			.mockResolvedValueOnce(makeJobResponse("job-1"))
			.mockRejectedValueOnce(new Error("second row exploded"))
			.mockResolvedValueOnce(makeJobResponse("job-3"));

		fireEvent.change(screen.getByLabelText("video_path values (one per line)"), {
			target: { value: "a.mkv\nb.mkv\nc.mkv" },
		});

		fireEvent.click(screen.getByRole("button", { name: "Submit Batch" }));

		await waitFor(() => {
			expect(screen.getByText(/second row exploded/)).toBeInTheDocument();
		});
		expect(screen.getByText(/submitted 1 \/ total 3/)).toBeInTheDocument();
		expect(submitJobWithParams).toHaveBeenCalledTimes(2);
	});

	it("submits repeat mode exactly repeatCount times", async () => {
		renderBatchPanel([]);

		vi.mocked(submitJob)
			.mockResolvedValueOnce(makeJobResponse("job-1"))
			.mockResolvedValueOnce(makeJobResponse("job-2"))
			.mockResolvedValueOnce(makeJobResponse("job-3"));

		fireEvent.change(screen.getByLabelText("Repeat count"), {
			target: { value: "3" },
		});
		fireEvent.click(screen.getByRole("button", { name: "Submit Batch" }));

		const workflow = useWorkflowStore.getState().exportWorkflow();

		await waitFor(() => {
			expect(submitJob).toHaveBeenCalledTimes(3);
		});
		expect(submitJobWithParams).not.toHaveBeenCalled();
		expect(submitJob).toHaveBeenNthCalledWith(1, workflow);
		expect(submitJob).toHaveBeenNthCalledWith(2, workflow);
		expect(submitJob).toHaveBeenNthCalledWith(3, workflow);
	});

	it("guards double-click submit so only one in-flight chain runs", async () => {
		renderBatchPanel([]);

		let resolveFirst: ((value: CreateJobResponse) => void) | undefined;
		const firstCall = new Promise<CreateJobResponse>((resolve) => {
			resolveFirst = resolve;
		});
		vi.mocked(submitJob).mockReturnValue(firstCall);

		const submitButton = screen.getByRole("button", { name: "Submit Batch" });
		fireEvent.click(submitButton);
		fireEvent.click(submitButton);

		expect(submitJob).toHaveBeenCalledTimes(1);

		resolveFirst?.(makeJobResponse("job-1"));
		await waitFor(() => {
			expect(screen.getByText("1 job submitted")).toBeInTheDocument();
		});
		expect(submitJob).toHaveBeenCalledTimes(1);
	});
});
