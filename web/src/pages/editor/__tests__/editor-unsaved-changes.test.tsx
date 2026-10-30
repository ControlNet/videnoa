import { beforeEach, describe, expect, it, vi } from "vitest";

vi.hoisted(() => {
	Object.defineProperty(window, "matchMedia", {
		writable: true,
		value: vi.fn().mockImplementation((query: string) => ({
			matches: false,
			media: query,
			onchange: null,
			addListener: vi.fn(),
			removeListener: vi.fn(),
			addEventListener: vi.fn(),
			removeEventListener: vi.fn(),
			dispatchEvent: vi.fn(),
		})),
	});
});

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { WorkflowEntry } from "@/api/client";
import { listPresets, listWorkflows, saveWorkflow } from "@/api/client";
import { TooltipProvider } from "@/components/ui/tooltip";
import { useUIStore } from "@/stores/ui-store";
import { selectIsDirty, useWorkflowStore } from "@/stores/workflow-store";
import { EditorToolbar } from "../EditorToolbar";
import { PresetGallery } from "../PresetGallery";

vi.mock("@/api/client", async (importOriginal) => ({
	...(await importOriginal<typeof import("@/api/client")>()),
	listPresets: vi.fn(),
	listWorkflows: vi.fn(),
	saveWorkflow: vi.fn(),
}));

vi.mock("@/components/shared/Toaster", () => ({
	toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() },
}));

vi.mock("react-router", () => ({ useNavigate: () => vi.fn() }));

vi.mock("../auto-layout", () => ({
	computeLayout: vi.fn(() => ({})),
}));

const SAVED: WorkflowEntry = {
	filename: "other.json",
	name: "Other Flow",
	description: "",
	workflow: {
		nodes: [{ id: "o1", node_type: "VideoInput", params: {} }],
		connections: [],
	},
	has_interface: false,
};

function editCanvas() {
	useWorkflowStore.getState().loadWorkflow({
		nodes: [{ id: "p1", node_type: "Print", params: { value: "x" } }],
		connections: [],
	});
	useWorkflowStore.getState().setCurrentFile({
		filename: "mine.json",
		name: "Mine",
		description: "",
	});
	useWorkflowStore.getState().updateNodeParams("p1", { value: "edited" });
	expect(selectIsDirty(useWorkflowStore.getState())).toBe(true);
}

beforeEach(() => {
	vi.clearAllMocks();
	useUIStore.setState({ activeModal: null });
	useWorkflowStore.getState().clear();
	vi.mocked(listPresets).mockResolvedValue([]);
	vi.mocked(listWorkflows).mockResolvedValue([SAVED]);
});

describe("unsaved changes", () => {
	it("asks before loading another workflow over unsaved changes", async () => {
		editCanvas();
		useUIStore.setState({ activeModal: "presets" });
		render(<PresetGallery />);

		fireEvent.click(await screen.findByText("Other Flow"));

		expect(
			screen.getByRole("dialog", { name: "Discard unsaved changes?" }),
		).toBeInTheDocument();
		expect(useWorkflowStore.getState().currentFile?.filename).toBe("mine.json");

		fireEvent.click(screen.getByRole("button", { name: "Keep editing" }));
		await waitFor(() =>
			expect(
				screen.queryByRole("dialog", { name: "Discard unsaved changes?" }),
			).toBeNull(),
		);
		expect(useWorkflowStore.getState().currentFile?.filename).toBe("mine.json");

		fireEvent.click(screen.getByText("Other Flow"));
		fireEvent.click(screen.getByRole("button", { name: "Discard changes" }));

		expect(useWorkflowStore.getState().currentFile?.filename).toBe("other.json");
		expect(useWorkflowStore.getState().nodes.map((n) => n.id)).toEqual(["o1"]);
	});

	it("loads without asking when the canvas is saved", async () => {
		useUIStore.setState({ activeModal: "presets" });
		render(<PresetGallery />);

		fireEvent.click(await screen.findByText("Other Flow"));

		expect(screen.queryByRole("dialog", { name: "Discard unsaved changes?" })).toBeNull();
		expect(useWorkflowStore.getState().currentFile?.filename).toBe("other.json");
	});

	it("asks before clearing unsaved changes, and a save makes the canvas clean", async () => {
		editCanvas();
		render(
			<TooltipProvider>
				<EditorToolbar onAutoLayout={vi.fn()} onFitView={vi.fn()} />
			</TooltipProvider>,
		);

		fireEvent.click(screen.getByRole("button", { name: "Clear" }));
		expect(
			screen.getByRole("dialog", { name: "Discard unsaved changes?" }),
		).toBeInTheDocument();
		expect(useWorkflowStore.getState().nodes).toHaveLength(1);
		fireEvent.click(screen.getByRole("button", { name: "Keep editing" }));

		vi.mocked(saveWorkflow).mockResolvedValue({
			filename: "mine.json",
			name: "Mine",
			description: "",
			workflow: { nodes: [], connections: [] },
			has_interface: false,
		});
		fireEvent.click(screen.getByRole("button", { name: "Save" }));
		await waitFor(() =>
			expect(selectIsDirty(useWorkflowStore.getState())).toBe(false),
		);

		fireEvent.click(screen.getByRole("button", { name: "Clear" }));
		expect(useWorkflowStore.getState().nodes).toHaveLength(0);
	});
});
