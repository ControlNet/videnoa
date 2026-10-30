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

import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { ApiError, saveWorkflow } from "@/api/client";
import { toast } from "@/components/shared/Toaster";
import { TooltipProvider } from "@/components/ui/tooltip";
import { useWorkflowStore } from "@/stores/workflow-store";
import { EditorToolbar } from "../EditorToolbar";

vi.mock("@/api/client", async (importOriginal) => ({
	...(await importOriginal<typeof import("@/api/client")>()),
	saveWorkflow: vi.fn(),
}));

vi.mock("@/components/shared/Toaster", () => ({
	toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() },
}));

vi.mock("react-router", () => ({ useNavigate: () => vi.fn() }));

const WORKFLOW = {
	nodes: [{ id: "p1", node_type: "Print", params: { value: "x" } }],
	connections: [],
};

function savedEntry(filename: string, name: string) {
	return {
		filename,
		name,
		description: "",
		workflow: WORKFLOW,
		has_interface: false,
	};
}

function renderToolbar() {
	return render(
		<TooltipProvider>
			<EditorToolbar onAutoLayout={vi.fn()} onFitView={vi.fn()} />
		</TooltipProvider>,
	);
}

async function saveAs(name: string) {
	fireEvent.click(screen.getByRole("button", { name: "Save" }));
	const input = await screen.findByPlaceholderText("Workflow name");
	fireEvent.change(input, { target: { value: name } });
	const popover = input.closest("[role='dialog']") as HTMLElement;
	fireEvent.click(within(popover).getByRole("button", { name: "Save" }));
}

beforeEach(() => {
	vi.clearAllMocks();
	useWorkflowStore.getState().loadWorkflow(WORKFLOW);
});

describe("EditorToolbar save", () => {
	it("saves a loaded workflow back to the file it came from", async () => {
		// A file copied on disk keeps the original's JSON name.
		useWorkflowStore.getState().setCurrentFile({
			filename: "copy.json",
			name: "Original",
			description: "d",
		});
		vi.mocked(saveWorkflow).mockResolvedValue(savedEntry("copy.json", "Original"));
		renderToolbar();

		fireEvent.click(screen.getByRole("button", { name: "Save" }));

		await waitFor(() => expect(saveWorkflow).toHaveBeenCalledTimes(1));
		expect(saveWorkflow).toHaveBeenCalledWith(
			"Original",
			"d",
			expect.objectContaining({ nodes: expect.any(Array) }),
			{ filename: "copy.json", overwrite: true },
		);
	});

	it("asks before Save as replaces an existing workflow", async () => {
		vi.mocked(saveWorkflow)
			.mockRejectedValueOnce(
				new ApiError(409, "exists", { error: "exists", code: "workflow_exists" }),
			)
			.mockResolvedValueOnce(savedEntry("Dup.json", "Dup"));
		renderToolbar();

		await saveAs("Dup");

		const dialog = await screen.findByRole("dialog", { name: "Replace workflow?" });
		expect(saveWorkflow).toHaveBeenCalledTimes(1);
		expect(vi.mocked(saveWorkflow).mock.calls[0][3]).toBeUndefined();
		expect(useWorkflowStore.getState().currentFile).toBeNull();

		fireEvent.click(screen.getByRole("button", { name: "Replace" }));

		await waitFor(() => expect(saveWorkflow).toHaveBeenCalledTimes(2));
		expect(vi.mocked(saveWorkflow).mock.calls[1][3]).toEqual({ overwrite: true });
		await waitFor(() =>
			expect(useWorkflowStore.getState().currentFile?.filename).toBe("Dup.json"),
		);
		expect(dialog).not.toBeInTheDocument();
	});

	it("explains that a preset's name cannot be used", async () => {
		vi.mocked(saveWorkflow).mockRejectedValueOnce(
			new ApiError(409, "preset", { error: "preset", code: "workflow_name_is_preset" }),
		);
		renderToolbar();

		await saveAs("interpolation-2x");

		await waitFor(() =>
			expect(toast.error).toHaveBeenCalledWith(
				"“interpolation-2x” is a preset's name. Save the workflow under another name.",
			),
		);
		expect(screen.queryByRole("dialog", { name: "Replace workflow?" })).toBeNull();
	});
});
