import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { listPresets, listWorkflows, submitJob, type WorkflowEntry } from "@/api/client";
import { toast } from "@/components/shared/Toaster";
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

interface Deferred<T> {
	promise: Promise<T>;
	resolve: (value: T) => void;
	reject: (reason?: unknown) => void;
}

function createDeferred<T>(): Deferred<T> {
	let resolve!: (value: T) => void;
	let reject!: (reason?: unknown) => void;
	const promise = new Promise<T>((res, rej) => {
		resolve = res;
		reject = rej;
	});
	return { promise, resolve, reject };
}

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

	it("clears the selection and shows the spinner again after reopening", async () => {
		vi.mocked(listWorkflows).mockResolvedValue([makeEntry(), multiParamEntry]);
		const props = { onOpenChange: vi.fn(), onSubmitted: vi.fn() };
		const view = render(<RunWorkflowDialog open={true} {...props} />);

		fireEvent.click(await screen.findByText("Multi-Param"));
		await screen.findByRole("button", { name: /Run/i });

		view.rerender(<RunWorkflowDialog open={false} {...props} />);
		vi.mocked(listWorkflows).mockReturnValue(new Promise(() => {}));
		view.rerender(<RunWorkflowDialog open={true} {...props} />);

		expect(screen.getByText("Loading workflows…")).toBeInTheDocument();
		expect(screen.queryByRole("button", { name: /Run/i })).not.toBeInTheDocument();
	});

	it("ignores a late workflow list from a closed open-cycle and submits the fresh graph", async () => {
		const firstLoad = createDeferred<WorkflowEntry[]>();
		const secondLoad = createDeferred<WorkflowEntry[]>();
		vi.mocked(listWorkflows)
			.mockReturnValueOnce(firstLoad.promise)
			.mockReturnValueOnce(secondLoad.promise);
		vi.mocked(submitJob).mockResolvedValue({
			id: "job-fresh",
			status: "queued",
			created_at: new Date().toISOString(),
		});

		const staleSharedEntry = makeEntry({
			filename: "pipeline.json",
			name: "Render Pipeline",
			description: "Stale graph",
			workflow: {
				nodes: [
					{ id: "wi", node_type: "WorkflowInput", params: {} },
					{ id: "legacy-node", node_type: "LegacyStep", params: { rev: 1 } },
					{ id: "wo", node_type: "WorkflowOutput", params: {} },
				],
				connections: [],
				interface: {
					inputs: [{ name: "greeting", port_type: "Str", default_value: "old" }],
					outputs: [],
				},
			},
		});
		const deletedEntry = makeEntry({
			filename: "deleted.json",
			name: "Deleted Workflow",
			description: "Removed before reopen",
		});
		const freshSharedEntry = makeEntry({
			filename: "pipeline.json",
			name: "Render Pipeline",
			description: "Fresh graph",
			workflow: {
				nodes: [
					{ id: "wi", node_type: "WorkflowInput", params: {} },
					{ id: "fresh-node", node_type: "FreshStep", params: { rev: 2 } },
					{ id: "wo", node_type: "WorkflowOutput", params: {} },
				],
				connections: [],
				interface: {
					inputs: [{ name: "greeting", port_type: "Str", default_value: "new" }],
					outputs: [],
				},
			},
		});

		const props = { onOpenChange: vi.fn(), onSubmitted: vi.fn() };
		const view = render(<RunWorkflowDialog open={true} {...props} />);
		expect(screen.getByText("Loading workflows…")).toBeInTheDocument();

		view.rerender(<RunWorkflowDialog open={false} {...props} />);
		view.rerender(<RunWorkflowDialog open={true} {...props} />);
		expect(listWorkflows).toHaveBeenCalledTimes(2);
		expect(screen.getByText("Loading workflows…")).toBeInTheDocument();

		await act(async () => {
			secondLoad.resolve([freshSharedEntry]);
			await secondLoad.promise;
		});

		expect(screen.queryByText("Loading workflows…")).not.toBeInTheDocument();
		expect(screen.getByText("Render Pipeline")).toBeInTheDocument();
		expect(screen.getByText("Fresh graph")).toBeInTheDocument();

		await act(async () => {
			firstLoad.resolve([staleSharedEntry, deletedEntry]);
			await firstLoad.promise;
		});

		expect(screen.queryByText("Deleted Workflow")).not.toBeInTheDocument();
		expect(screen.queryByText("Stale graph")).not.toBeInTheDocument();
		expect(screen.getByText("Fresh graph")).toBeInTheDocument();

		fireEvent.click(screen.getByText("Render Pipeline"));
		fireEvent.click(await screen.findByRole("button", { name: /Run/i }));

		await waitFor(() => {
			expect(submitJob).toHaveBeenCalledTimes(1);
		});

		const submitted = vi.mocked(submitJob).mock.calls[0][0];
		expect(
			submitted.nodes.some((n) => n.id === "fresh-node" && n.node_type === "FreshStep"),
		).toBe(true);
		expect(submitted.nodes.some((n) => n.id === "legacy-node")).toBe(false);
		const wiNode = submitted.nodes.find((n) => n.node_type === "WorkflowInput");
		expect(wiNode?.params).toEqual(expect.objectContaining({ greeting: "new" }));
	});

	it("keeps loading active and suppresses error toasts when a cancelled request rejects before the reopened request settles", async () => {
		const firstLoad = createDeferred<WorkflowEntry[]>();
		const secondLoad = createDeferred<WorkflowEntry[]>();
		vi.mocked(listWorkflows)
			.mockReturnValueOnce(firstLoad.promise)
			.mockReturnValueOnce(secondLoad.promise);

		const props = { onOpenChange: vi.fn(), onSubmitted: vi.fn() };
		const view = render(<RunWorkflowDialog open={true} {...props} />);
		view.rerender(<RunWorkflowDialog open={false} {...props} />);
		view.rerender(<RunWorkflowDialog open={true} {...props} />);

		expect(screen.getByText("Loading workflows…")).toBeInTheDocument();

		await act(async () => {
			firstLoad.reject(new Error("cancelled request failed"));
			await firstLoad.promise.catch(() => {});
		});

		expect(toast.error).not.toHaveBeenCalled();
		expect(screen.getByText("Loading workflows…")).toBeInTheDocument();
		expect(
			screen.queryByText(/No workflows with input ports found/),
		).not.toBeInTheDocument();

		await act(async () => {
			secondLoad.resolve([makeEntry()]);
			await secondLoad.promise;
		});

		expect(screen.queryByText("Loading workflows…")).not.toBeInTheDocument();
		expect(screen.getByText("Greeting Workflow")).toBeInTheDocument();
	});

	it("keeps loading active when a cancelled request resolves before the reopened request settles", async () => {
		const firstLoad = createDeferred<WorkflowEntry[]>();
		const secondLoad = createDeferred<WorkflowEntry[]>();
		vi.mocked(listWorkflows)
			.mockReturnValueOnce(firstLoad.promise)
			.mockReturnValueOnce(secondLoad.promise);

		const props = { onOpenChange: vi.fn(), onSubmitted: vi.fn() };
		const view = render(<RunWorkflowDialog open={true} {...props} />);
		view.rerender(<RunWorkflowDialog open={false} {...props} />);
		view.rerender(<RunWorkflowDialog open={true} {...props} />);

		await act(async () => {
			firstLoad.resolve([
				makeEntry({ filename: "stale.json", name: "Stale Early Workflow" }),
			]);
			await firstLoad.promise;
		});

		expect(screen.getByText("Loading workflows…")).toBeInTheDocument();
		expect(screen.queryByText("Stale Early Workflow")).not.toBeInTheDocument();

		await act(async () => {
			secondLoad.resolve([
				makeEntry({ filename: "fresh.json", name: "Fresh Final Workflow" }),
			]);
			await secondLoad.promise;
		});

		expect(screen.queryByText("Loading workflows…")).not.toBeInTheDocument();
		expect(screen.getByText("Fresh Final Workflow")).toBeInTheDocument();
		expect(screen.queryByText("Stale Early Workflow")).not.toBeInTheDocument();
	});

	it("clears previous session workflows when a reopened load fails", async () => {
		const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
		try {
			vi.mocked(listWorkflows).mockResolvedValueOnce([
				makeEntry({ filename: "old.json", name: "Old Session Workflow" }),
			]);

			const props = { onOpenChange: vi.fn(), onSubmitted: vi.fn() };
			const view = render(<RunWorkflowDialog open={true} {...props} />);
			expect(await screen.findByText("Old Session Workflow")).toBeInTheDocument();

			view.rerender(<RunWorkflowDialog open={false} {...props} />);
			vi.mocked(listWorkflows).mockRejectedValueOnce(new Error("server unavailable"));
			view.rerender(<RunWorkflowDialog open={true} {...props} />);

			await waitFor(() => {
				expect(toast.error).toHaveBeenCalledWith("Failed to load workflows");
			});
			expect(screen.queryByText("Loading workflows…")).not.toBeInTheDocument();
			expect(screen.queryByText("Old Session Workflow")).not.toBeInTheDocument();
		} finally {
			consoleErrorSpy.mockRestore();
		}
	});
});
