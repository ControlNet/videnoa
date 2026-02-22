import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { listPresets, listWorkflows, submitJob, type WorkflowEntry } from "@/api/client";
import { i18n, initializeI18n } from "@/i18n";
import { RunWorkflowDialog } from "../RunWorkflowDialog";

vi.mock("@/api/client", () => ({
	listPresets: vi.fn(),
	listWorkflows: vi.fn(),
	submitJob: vi.fn(),
}));

vi.mock("@/components/shared/Toaster", () => ({
	toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() },
}));

function makeEntry(overrides: Partial<WorkflowEntry> = {}): WorkflowEntry {
	return {
		filename: "greet.json",
		name: "Greeting Workflow",
		description: "Says hello",
		workflow: {
			nodes: [
				{ id: "wi", node_type: "WorkflowInput", params: {} },
				{ id: "wo", node_type: "WorkflowOutput", params: {} },
			],
			connections: [
				{
					from_node: "wi",
					from_port: "greeting",
					to_node: "wo",
					to_port: "greeting",
					port_type: "Str",
				},
			],
			interface: {
				inputs: [{ name: "greeting", port_type: "Str" }],
				outputs: [{ name: "greeting", port_type: "Str" }],
			},
		},
		has_interface: true,
		...overrides,
	};
}

const multiParamEntry: WorkflowEntry = makeEntry({
	filename: "multi.json",
	name: "Multi-Param",
	description: "Multiple param types",
	workflow: {
		nodes: [
			{ id: "wi", node_type: "WorkflowInput", params: {} },
			{ id: "wo", node_type: "WorkflowOutput", params: {} },
		],
		connections: [],
		interface: {
			inputs: [
				{ name: "label", port_type: "Str" },
				{ name: "count", port_type: "Int", default_value: 5 },
				{ name: "verbose", port_type: "Bool", default_value: false },
			],
			outputs: [],
		},
	},
});

beforeEach(async () => {
	initializeI18n();
	await i18n.changeLanguage("en");

	vi.clearAllMocks();
	vi.mocked(listPresets).mockResolvedValue([]);
});

describe("RunWorkflowDialog", () => {
	it("renders empty state when no workflows with interfaces", async () => {
		vi.mocked(listWorkflows).mockResolvedValue([]);

		render(
			<RunWorkflowDialog
				open={true}
				onOpenChange={vi.fn()}
				onSubmitted={vi.fn()}
			/>,
		);

		await waitFor(() => {
			expect(
				screen.getByText(/No workflows with input ports found/),
			).toBeInTheDocument();
		});
	});

	it("filters out workflows without has_interface", async () => {
		vi.mocked(listWorkflows).mockResolvedValue([
			makeEntry({ has_interface: false, name: "No Interface" }),
		]);

		render(
			<RunWorkflowDialog
				open={true}
				onOpenChange={vi.fn()}
				onSubmitted={vi.fn()}
			/>,
		);

		await waitFor(() => {
			expect(
				screen.getByText(/No workflows with input ports found/),
			).toBeInTheDocument();
		});
		expect(screen.queryByText("No Interface")).not.toBeInTheDocument();
	});

	it("shows workflow list and allows selection", async () => {
		vi.mocked(listWorkflows).mockResolvedValue([makeEntry(), multiParamEntry]);

		render(
			<RunWorkflowDialog
				open={true}
				onOpenChange={vi.fn()}
				onSubmitted={vi.fn()}
			/>,
		);

		await waitFor(() => {
			expect(screen.getByText("Greeting Workflow")).toBeInTheDocument();
		});
		expect(screen.getByText("Multi-Param")).toBeInTheDocument();

		fireEvent.click(screen.getByText("Multi-Param"));

		await waitFor(() => {
			expect(screen.getByRole("button", { name: /Run/i })).toBeInTheDocument();
		});
	});

	it("shows dynamic form fields after selection", async () => {
		vi.mocked(listWorkflows).mockResolvedValue([multiParamEntry]);

		render(
			<RunWorkflowDialog
				open={true}
				onOpenChange={vi.fn()}
				onSubmitted={vi.fn()}
			/>,
		);

		await waitFor(() => {
			expect(screen.getByText("Multi-Param")).toBeInTheDocument();
		});

		fireEvent.click(screen.getByText("Multi-Param"));

		await waitFor(() => {
			expect(screen.getByText("Parameters")).toBeInTheDocument();
		});

		expect(screen.getByText("label")).toBeInTheDocument();
		expect(screen.getByText("count")).toBeInTheDocument();
		expect(screen.getByText("verbose")).toBeInTheDocument();
	});

	it("submit button injects params into WorkflowInput and calls submitJob", async () => {
		vi.mocked(listWorkflows).mockResolvedValue([makeEntry()]);
		vi.mocked(submitJob).mockResolvedValue({
			id: "job-1",
			status: "queued",
			created_at: new Date().toISOString(),
		});

		const onSubmitted = vi.fn();
		const onOpenChange = vi.fn();

		render(
			<RunWorkflowDialog
				open={true}
				onOpenChange={onOpenChange}
				onSubmitted={onSubmitted}
			/>,
		);

		await waitFor(() => {
			expect(screen.getByText("Greeting Workflow")).toBeInTheDocument();
		});

		fireEvent.click(screen.getByText("Greeting Workflow"));

		await waitFor(() => {
			expect(screen.getByRole("button", { name: /Run/i })).toBeInTheDocument();
		});

		fireEvent.click(screen.getByRole("button", { name: /Run/i }));

		await waitFor(() => {
			expect(submitJob).toHaveBeenCalledTimes(1);
		});

		const submitted = vi.mocked(submitJob).mock.calls[0][0];
		const wiNode = submitted.nodes.find(
			(n: { node_type: string }) => n.node_type === "WorkflowInput",
		);
		expect(wiNode).toBeDefined();
		expect(wiNode?.params).toEqual(expect.objectContaining({ greeting: "" }));
		expect(onOpenChange).toHaveBeenCalledWith(false);
		expect(onSubmitted).toHaveBeenCalled();
	});

	it("shows loading spinner while fetching", async () => {
		vi.mocked(listWorkflows).mockReturnValue(new Promise(() => {}));

		render(
			<RunWorkflowDialog
				open={true}
				onOpenChange={vi.fn()}
				onSubmitted={vi.fn()}
			/>,
		);

		await waitFor(() => {
			expect(document.querySelector(".animate-spin")).toBeInTheDocument();
		});
		expect(screen.getByText("Loading workflows…")).toBeInTheDocument();
	});
});
