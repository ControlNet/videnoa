import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../../api/client";
import { useJobStore } from "../job-store";

type DebugCallback = Parameters<typeof api.subscribeToProgress>[3];

function jsonResponse(body: unknown, status = 200): Response {
	return new Response(JSON.stringify(body), {
		status,
		headers: { "Content-Type": "application/json" },
	});
}

function emitPrintPreview(jobId: string): void {
	let emitDebug: DebugCallback | undefined;
	vi.spyOn(api, "subscribeToProgress").mockImplementation(
		(_jobId, _onProgress, _onClose, onDebug) => {
			emitDebug = onDebug;
			return { close: vi.fn() };
		},
	);

	useJobStore.getState().subscribeToJob(jobId);
	emitDebug?.({
		type: "node_debug_value",
		node_id: "print-1",
		node_type: "Print",
		value_preview: "x".repeat(512),
		truncated: true,
		preview_max_chars: 512,
	});
	useJobStore.getState().unsubscribeFromJob();
}

beforeEach(() => {
	vi.stubGlobal("fetch", vi.fn());
	useJobStore.setState({
		jobs: [],
		refreshError: null,
		activeJobId: null,
		activeProgress: null,
		runtimePreviewsByNodeId: {},
		runtimePreviewsByJobId: {},
		wsCleanup: null,
	});
});

afterEach(() => {
	vi.restoreAllMocks();
});

describe("runtimePreviewsByJobId retention", () => {
	it("keeps a finished job's Print previews after its socket closes", () => {
		emitPrintPreview("j1");

		expect(useJobStore.getState().runtimePreviewsByJobId.j1).toBeDefined();
	});

	it("drops a job's Print previews when its history is deleted", async () => {
		emitPrintPreview("j1");
		const mockFetch = vi.mocked(fetch);
		mockFetch.mockResolvedValueOnce(new Response(null, { status: 204 }));
		mockFetch.mockResolvedValueOnce(jsonResponse([]));

		await useJobStore.getState().deleteJobHistory("j1");
		await vi.waitFor(() => expect(mockFetch).toHaveBeenCalledTimes(2));

		expect(useJobStore.getState().jobs).toEqual([]);
		expect(useJobStore.getState().runtimePreviewsByJobId.j1).toBeUndefined();
	});

	it("keeps a job's Print previews when deleting its history fails", async () => {
		emitPrintPreview("j1");
		vi.mocked(fetch).mockResolvedValueOnce(jsonResponse({ error: "busy" }, 500));

		await expect(useJobStore.getState().deleteJobHistory("j1")).rejects.toThrow("busy");

		expect(useJobStore.getState().runtimePreviewsByJobId.j1).toBeDefined();
	});
});
