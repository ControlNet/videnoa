import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { TooltipProvider } from "@/components/ui/tooltip";
import { i18n, initializeI18n } from "@/i18n";
import { useJobStore } from "@/stores/job-store";
import { useWorkflowStore } from "@/stores/workflow-store";
import { EditorToolbar } from "../EditorToolbar";
import { RunFromEditorDialog } from "../RunFromEditorDialog";

const navigate = vi.hoisted(() => vi.fn());

vi.mock("react-router", () => ({
	useNavigate: () => navigate,
}));

vi.mock("@/api/client", () => ({
	saveWorkflow: vi.fn(),
}));

vi.mock("@/components/shared/Toaster", () => ({
	toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() },
}));

beforeEach(async () => {
	initializeI18n();
	await i18n.changeLanguage("en");
	vi.clearAllMocks();
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
		currentFile: null,
		submittedJobId: null,
	});
	useJobStore.setState({
		submitJob: vi.fn().mockResolvedValue("job-from-canvas"),
	});
});

describe("editor submit paths record the canvas job", () => {
	it("records the job started from the toolbar Run button", async () => {
		render(
			<TooltipProvider>
				<EditorToolbar onAutoLayout={vi.fn()} onFitView={vi.fn()} />
			</TooltipProvider>,
		);

		fireEvent.click(screen.getByRole("button", { name: "Run Workflow" }));

		await waitFor(() => {
			expect(navigate).toHaveBeenCalledWith("/jobs");
		});
		expect(useWorkflowStore.getState().submittedJobId).toBe("job-from-canvas");
	});

	it("records the job started from the run-parameters dialog", async () => {
		render(
			<RunFromEditorDialog open onOpenChange={vi.fn()} inputs={[]} />,
		);

		fireEvent.click(screen.getByRole("button", { name: "Run" }));

		await waitFor(() => {
			expect(navigate).toHaveBeenCalledWith("/jobs");
		});
		expect(useWorkflowStore.getState().submittedJobId).toBe("job-from-canvas");
	});
});
