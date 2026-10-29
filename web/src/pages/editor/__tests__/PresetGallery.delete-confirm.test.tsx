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

import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { WorkflowEntry } from "@/api/client";
import { deleteWorkflow, listPresets, listWorkflows } from "@/api/client";
import { toast } from "@/components/shared/Toaster";
import { i18n, initializeI18n } from "@/i18n";
import { useUIStore } from "@/stores/ui-store";
import { useWorkflowStore } from "@/stores/workflow-store";
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
	useUIStore.setState({ activeModal: "presets" });
	useWorkflowStore.setState({
		nodes: [],
		edges: [],
		past: [],
		future: [],
		currentFile: null,
	});
	vi.mocked(listPresets).mockResolvedValue([]);
	vi.mocked(listWorkflows).mockResolvedValue([WORKFLOW_FIXTURE]);
	vi.mocked(deleteWorkflow).mockResolvedValue(undefined);
});

async function openDeleteConfirm(): Promise<HTMLElement> {
	render(<PresetGallery />);
	await waitFor(() => {
		expect(screen.getByText("My Custom Flow")).toBeInTheDocument();
	});
	fireEvent.click(
		screen.getByRole("button", { name: "Delete workflow My Custom Flow" }),
	);
	return screen.getByRole("dialog", { name: "Delete workflow?" });
}

function expectGalleryStillOpen(): void {
	expect(useUIStore.getState().activeModal).toBe("presets");
	expect(screen.getByRole("dialog", { name: "Load Workflow" })).toBeInTheDocument();
	expect(screen.getByText("My Custom Flow")).toBeInTheDocument();
}

describe("PresetGallery delete confirmation", () => {
	it("asks for confirmation naming the workflow instead of deleting on the first click", async () => {
		const confirm = await openDeleteConfirm();
		await act(async () => {
			await new Promise((resolve) => setTimeout(resolve, 0));
		});

		expect(confirm).toHaveTextContent("My Custom Flow");
		expect(deleteWorkflow).not.toHaveBeenCalled();
		expect(useWorkflowStore.getState().nodes).toEqual([]);
		// The confirmation is modal, so the gallery underneath is aria-hidden.
		expect(useUIStore.getState().activeModal).toBe("presets");
		expect(screen.getByText("Load Workflow")).toBeInTheDocument();
		expect(screen.getByText("My Custom Flow")).toBeInTheDocument();
	});

	it("deletes the pending workflow only after confirming", async () => {
		await openDeleteConfirm();

		fireEvent.click(screen.getByRole("button", { name: "Delete" }));

		await waitFor(() => {
			expect(deleteWorkflow).toHaveBeenCalledWith("my-flow.json");
		});
		await waitFor(() => {
			expect(screen.queryByText("My Custom Flow")).not.toBeInTheDocument();
		});
		expect(deleteWorkflow).toHaveBeenCalledTimes(1);
		expect(toast.success).toHaveBeenCalledWith("Workflow deleted");
		expect(screen.queryByRole("dialog", { name: "Delete workflow?" })).not.toBeInTheDocument();
		expect(useUIStore.getState().activeModal).toBe("presets");
		expect(useWorkflowStore.getState().nodes).toEqual([]);
	});

	it("keeps the workflow when the confirmation is cancelled", async () => {
		await openDeleteConfirm();

		fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

		await waitFor(() => {
			expect(screen.queryByRole("dialog", { name: "Delete workflow?" })).not.toBeInTheDocument();
		});
		expect(deleteWorkflow).not.toHaveBeenCalled();
		expect(useWorkflowStore.getState().nodes).toEqual([]);
		expectGalleryStillOpen();
	});

	it("closes only the confirmation on Escape", async () => {
		await openDeleteConfirm();

		fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });

		await waitFor(() => {
			expect(screen.queryByRole("dialog", { name: "Delete workflow?" })).not.toBeInTheDocument();
		});
		expect(deleteWorkflow).not.toHaveBeenCalled();
		expectGalleryStillOpen();
	});

	it("closes only the confirmation on an outside click", async () => {
		await openDeleteConfirm();
		// Radix registers its outside-pointer listener on the next tick.
		await act(async () => {
			await new Promise((resolve) => setTimeout(resolve, 0));
		});

		fireEvent.pointerDown(document.body);

		await waitFor(() => {
			expect(screen.queryByRole("dialog", { name: "Delete workflow?" })).not.toBeInTheDocument();
		});
		expect(deleteWorkflow).not.toHaveBeenCalled();
		expectGalleryStillOpen();
	});

	it("drops a pending confirmation when the gallery closes", async () => {
		await openDeleteConfirm();

		act(() => {
			useUIStore.getState().closeModal();
		});
		act(() => {
			useUIStore.getState().openModal("presets");
		});

		await waitFor(() => {
			expect(screen.getByText("My Custom Flow")).toBeInTheDocument();
		});
		expect(screen.queryByRole("dialog", { name: "Delete workflow?" })).not.toBeInTheDocument();
		expect(deleteWorkflow).not.toHaveBeenCalled();
	});
});
