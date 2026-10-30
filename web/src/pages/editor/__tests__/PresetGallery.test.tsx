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
import { listPresets, listWorkflows } from "@/api/client";
import { i18n, initializeI18n } from "@/i18n";
import { useUIStore } from "@/stores/ui-store";
import { useWorkflowStore } from "@/stores/workflow-store";
import type { Preset } from "@/types";
import { PresetGallery } from "../PresetGallery";

vi.mock("@/api/client", () => ({
	listPresets: vi.fn(),
	listWorkflows: vi.fn(),
	deleteWorkflow: vi.fn(),
}));

vi.mock("@/components/shared/Toaster", () => ({
	toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() },
}));

vi.mock("../auto-layout", () => ({
	computeLayout: vi.fn(() => ({})),
}));

const PRESET_FIXTURE: Preset = {
	id: "preset-1",
	name: "Anime 2x Upscale",
	description: "Upscale anime by 2x",
	workflow: {
		nodes: [
			{ id: "input", node_type: "VideoInput", params: {} },
			{ id: "sr", node_type: "SuperResolution", params: { scale: 2 } },
			{ id: "output", node_type: "VideoOutput", params: {} },
		],
		connections: [
			{
				from_node: "input",
				from_port: "frames",
				to_node: "sr",
				to_port: "frames",
				port_type: "VideoFrames",
			},
			{
				from_node: "sr",
				from_port: "frames",
				to_node: "output",
				to_port: "frames",
				port_type: "VideoFrames",
			},
		],
	},
};

const WORKFLOW_FIXTURE: WorkflowEntry = {
	filename: "my-flow.json",
	name: "My Custom Flow",
	description: "A saved workflow",
	workflow: {
		nodes: [{ id: "n1", node_type: "VideoInput", params: {} }],
		connections: [],
	},
	has_interface: false,
};

beforeEach(async () => {
	initializeI18n();
	await i18n.changeLanguage("en");

	vi.clearAllMocks();

	useUIStore.setState({ activeModal: null });
	useWorkflowStore.setState({
		nodes: [
			{
				id: "n1",
				type: "pipeline",
				position: { x: 0, y: 0 },
				data: { nodeType: "VideoInput", params: {} },
			},
		],
		edges: [],
		past: [],
		future: [],
	});
});

describe("PresetGallery", () => {
	it("does not render dialog content when modal is not presets", () => {
		useUIStore.setState({ activeModal: null });

		render(<PresetGallery />);

		expect(screen.queryByText("Load Workflow")).not.toBeInTheDocument();
	});

	it("renders presets and workflows when open", async () => {
		useUIStore.setState({ activeModal: "presets" });
		vi.mocked(listPresets).mockResolvedValue([PRESET_FIXTURE]);
		vi.mocked(listWorkflows).mockResolvedValue([WORKFLOW_FIXTURE]);

		render(<PresetGallery />);

		await waitFor(() => {
			expect(screen.getByText("Built-in Presets")).toBeInTheDocument();
		});
		expect(screen.getByText("Saved Workflows")).toBeInTheDocument();
	});

	it("shows preset names", async () => {
		useUIStore.setState({ activeModal: "presets" });
		vi.mocked(listPresets).mockResolvedValue([PRESET_FIXTURE]);
		vi.mocked(listWorkflows).mockResolvedValue([]);

		render(<PresetGallery />);

		await waitFor(() => {
			expect(screen.getByText("Anime 2x Upscale")).toBeInTheDocument();
		});
		expect(screen.getByText("Upscale anime by 2x")).toBeInTheDocument();
	});

	it("shows workflow names and delete button", async () => {
		useUIStore.setState({ activeModal: "presets" });
		vi.mocked(listPresets).mockResolvedValue([]);
		vi.mocked(listWorkflows).mockResolvedValue([WORKFLOW_FIXTURE]);

		render(<PresetGallery />);

		await waitFor(() => {
			expect(screen.getByText("My Custom Flow")).toBeInTheDocument();
		});

		const deleteBtn = screen.getByRole("button", {
			name: "Delete workflow My Custom Flow",
		});
		expect(deleteBtn).toBeInTheDocument();
	});

	it("dialog title is Load Workflow", async () => {
		useUIStore.setState({ activeModal: "presets" });
		vi.mocked(listPresets).mockResolvedValue([]);
		vi.mocked(listWorkflows).mockResolvedValue([]);

		render(<PresetGallery />);

		await waitFor(() => {
			expect(screen.getByText("Load Workflow")).toBeInTheDocument();
		});
	});

	it("shows empty messages when no presets or workflows", async () => {
		useUIStore.setState({ activeModal: "presets" });
		vi.mocked(listPresets).mockResolvedValue([]);
		vi.mocked(listWorkflows).mockResolvedValue([]);

		render(<PresetGallery />);

		await waitFor(() => {
			expect(screen.getByText("No presets available")).toBeInTheDocument();
		});
		expect(screen.getByText("No saved workflows yet")).toBeInTheDocument();
	});

	// A failed list used to look exactly like an empty one.
	it("shows a load error with a retry instead of empty lists", async () => {
		useUIStore.setState({ activeModal: "presets" });
		vi.mocked(listPresets).mockRejectedValueOnce(new Error("HTTP 500"));
		vi.mocked(listWorkflows).mockResolvedValue([WORKFLOW_FIXTURE]);

		render(<PresetGallery />);

		await waitFor(() => {
			expect(screen.getByRole("alert")).toHaveTextContent(
				"Could not load the built-in presets",
			);
		});
		expect(screen.queryByText("No presets available")).not.toBeInTheDocument();
		// The other list still loads.
		expect(screen.getByText("My Custom Flow")).toBeInTheDocument();

		vi.mocked(listPresets).mockResolvedValueOnce([PRESET_FIXTURE]);
		fireEvent.click(screen.getByRole("button", { name: "Retry" }));

		await waitFor(() => {
			expect(screen.getByText("Anime 2x Upscale")).toBeInTheDocument();
		});
		expect(screen.queryByRole("alert")).not.toBeInTheDocument();
	});

	it("shows a load error for saved workflows", async () => {
		useUIStore.setState({ activeModal: "presets" });
		vi.mocked(listPresets).mockResolvedValue([PRESET_FIXTURE]);
		vi.mocked(listWorkflows).mockRejectedValueOnce(new Error("HTTP 401"));

		render(<PresetGallery />);

		await waitFor(() => {
			expect(screen.getByRole("alert")).toHaveTextContent(
				"Could not load saved workflows",
			);
		});
		expect(screen.queryByText("No saved workflows yet")).not.toBeInTheDocument();
		expect(screen.getByText("Anime 2x Upscale")).toBeInTheDocument();
	});
});
