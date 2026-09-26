import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../../api/client";
import type { JobResponse, ProgressUpdate } from "../../types";
import { useJobStore } from "../job-store";

function makeJobResponse(overrides: Partial<JobResponse> = {}): JobResponse {
	return {
		id: "j1",
		status: "completed",
		progress: null,
		created_at: "2025-01-01T00:00:00Z",
		started_at: null,
		completed_at: null,
		error: null,
		workflow_name: "Manual Workflow",
		workflow_source: "api_jobs",
		params: null,
		rerun_of_job_id: null,
		duration_ms: null,
		...overrides,
	};
}

function jsonResponse(body: unknown, status = 200): Response {
	return new Response(JSON.stringify(body), {
		status,
		headers: { "Content-Type": "application/json" },
	});
}

beforeEach(() => {
	vi.stubGlobal("fetch", vi.fn());
	useJobStore.setState({
		jobs: [],
		refreshError: null,
		activeJobId: null,
		activeProgress: null,
		runtimePreviewsByNodeId: {},
		wsCleanup: null,
	});
});

afterEach(() => {
	vi.restoreAllMocks();
});

describe("initial state", () => {
	it("has empty defaults", () => {
		const state = useJobStore.getState();
		expect(state.jobs).toEqual([]);
		expect(state.activeJobId).toBeNull();
		expect(state.activeProgress).toBeNull();
		expect(state.runtimePreviewsByNodeId).toEqual({});
	});
});

describe("fetchJobs", () => {
	it("populates jobs from API response", async () => {
		const jobs: JobResponse[] = [
			makeJobResponse({ id: "j1", status: "completed" }),
			makeJobResponse({
				id: "j2",
				status: "running",
				workflow_name: "Named Workflow",
				workflow_source: "api_run_workflows",
				params: { quality: "high", retries: 2 },
				rerun_of_job_id: "source-job",
				duration_ms: 1234,
			}),
		];
		vi.mocked(fetch).mockResolvedValueOnce(jsonResponse(jobs));

		await useJobStore.getState().fetchJobs();

		const state = useJobStore.getState();
		expect(state.jobs).toHaveLength(2);
		expect(state.jobs[0].id).toBe("j1");
		expect(state.jobs[1].id).toBe("j2");
		expect(state.jobs[0].status).toBe("completed");
		expect(state.jobs[1].status).toBe("running");
		expect(state.jobs[1].workflow_name).toBe("Named Workflow");
		expect(state.jobs[1].workflow_source).toBe("api_run_workflows");
		expect(state.jobs[1].params).toEqual({ quality: "high", retries: 2 });
		expect(state.jobs[1].rerun_of_job_id).toBe("source-job");
		expect(state.jobs[1].duration_ms).toBe(1234);
	});

	it("replaces previous jobs on re-fetch", async () => {
		useJobStore.setState({
			jobs: [
				{
					id: "old",
					status: "completed",
					progress: null,
					created_at: "",
					started_at: null,
					completed_at: null,
					error: null,
					workflow_name: "Old Workflow",
					workflow_source: "api_jobs",
					params: null,
					rerun_of_job_id: null,
					duration_ms: null,
				},
			],
		});

		vi.mocked(fetch).mockResolvedValueOnce(
			jsonResponse([makeJobResponse({ id: "new" })]),
		);

		await useJobStore.getState().fetchJobs();

		expect(useJobStore.getState().jobs).toHaveLength(1);
		expect(useJobStore.getState().jobs[0].id).toBe("new");
	});

	it("propagates errors from fetch", async () => {
		vi.mocked(fetch).mockResolvedValueOnce(
			new Response("Internal Server Error", {
				status: 500,
				statusText: "Internal Server Error",
			}),
		);

		await expect(useJobStore.getState().fetchJobs()).rejects.toThrow();
	});

	it("propagates network errors", async () => {
		vi.mocked(fetch).mockRejectedValueOnce(new TypeError("Failed to fetch"));

		await expect(useJobStore.getState().fetchJobs()).rejects.toThrow(
			"Failed to fetch",
		);
	});
});

describe("submitJob", () => {
	it("posts workflow and returns job id", async () => {
		const mockFetch = vi.mocked(fetch);

		mockFetch.mockResolvedValueOnce(
			jsonResponse(
				{ id: "new-job", status: "queued", created_at: "2025-01-01T00:00:00Z" },
				200,
			),
		);
		mockFetch.mockResolvedValueOnce(
			jsonResponse([makeJobResponse({ id: "new-job", status: "queued" })]),
		);

		const workflow = { nodes: [], connections: [] };
		const id = await useJobStore.getState().submitJob(workflow);

		expect(id).toBe("new-job");
		expect(mockFetch).toHaveBeenCalledTimes(2);

		const [url, init] = mockFetch.mock.calls[0];
		expect(url).toBe("/api/jobs");
		expect(init).toMatchObject({ method: "POST" });
	});

	it("refreshes jobs list after submit", async () => {
		const mockFetch = vi.mocked(fetch);
		mockFetch.mockResolvedValueOnce(
			jsonResponse({
				id: "j-new",
				status: "queued",
				created_at: "2025-01-01T00:00:00Z",
			}),
		);
		mockFetch.mockResolvedValueOnce(
			jsonResponse([
				makeJobResponse({ id: "j-new", status: "queued" }),
				makeJobResponse({ id: "j-old", status: "completed" }),
			]),
		);

		await useJobStore.getState().submitJob({ nodes: [], connections: [] });

		await vi.waitFor(() => expect(useJobStore.getState().jobs).toHaveLength(2));
	});
});

describe("runByName", () => {
	it("submits named workflow and refreshes jobs", async () => {
		const mockFetch = vi.mocked(fetch);
		mockFetch.mockResolvedValueOnce(
			jsonResponse(
				{ id: "run-job", status: "queued", created_at: "2025-01-01T00:00:00Z" },
				201,
			),
		);
		mockFetch.mockResolvedValueOnce(
			jsonResponse([makeJobResponse({ id: "run-job", workflow_name: "named-run" })]),
		);

		const id = await useJobStore
			.getState()
			.runByName("named-run", { quality: "high", retries: 2 });

		expect(id).toBe("run-job");
		expect(mockFetch).toHaveBeenCalledTimes(2);
		expect(mockFetch.mock.calls[0][0]).toBe("/api/run");
		expect(mockFetch.mock.calls[0][1]).toMatchObject({ method: "POST" });
		expect(JSON.parse(String(mockFetch.mock.calls[0][1]?.body))).toEqual({
			workflow_name: "named-run",
			params: { quality: "high", retries: 2 },
		});
	});
});

describe("rerunJob", () => {
	it("calls rerun endpoint and refreshes jobs", async () => {
		const mockFetch = vi.mocked(fetch);
		mockFetch.mockResolvedValueOnce(
			jsonResponse(
				{ id: "rerun-job", status: "queued", created_at: "2025-01-01T00:00:00Z" },
				201,
			),
		);
		mockFetch.mockResolvedValueOnce(jsonResponse([makeJobResponse({ id: "rerun-job" })]));

		const id = await useJobStore.getState().rerunJob("source-job");

		expect(id).toBe("rerun-job");
		expect(mockFetch.mock.calls[0][0]).toBe("/api/jobs/source-job/rerun");
		expect(mockFetch.mock.calls[0][1]).toEqual({ method: "POST" });
		expect(mockFetch).toHaveBeenCalledTimes(2);
	});
});

describe("deleteJobHistory", () => {
	it("calls delete history endpoint and refreshes jobs", async () => {
		const mockFetch = vi.mocked(fetch);
		mockFetch.mockResolvedValueOnce(new Response(null, { status: 204 }));
		mockFetch.mockResolvedValueOnce(jsonResponse([]));

		await useJobStore.getState().deleteJobHistory("j1");

		expect(mockFetch).toHaveBeenCalledTimes(2);
		expect(mockFetch.mock.calls[0][0]).toBe("/api/jobs/j1");
		expect(mockFetch.mock.calls[0][1]).toEqual({ method: "DELETE" });
		expect(useJobStore.getState().jobs).toEqual([]);
	});
});

describe("cancelJob", () => {
	it("delegates to delete history and refreshes jobs", async () => {
		const mockFetch = vi.mocked(fetch);

		mockFetch.mockResolvedValueOnce(new Response(null, { status: 204 }));
		mockFetch.mockResolvedValueOnce(
			jsonResponse([makeJobResponse({ id: "j1", status: "cancelled" })]),
		);

		await useJobStore.getState().cancelJob("j1");

		expect(mockFetch).toHaveBeenCalledTimes(2);

		const [url, init] = mockFetch.mock.calls[0];
		expect(url).toBe("/api/jobs/j1");
		expect(init).toMatchObject({ method: "DELETE" });

		await vi.waitFor(() => expect(useJobStore.getState().jobs).toHaveLength(1));
		expect(useJobStore.getState().jobs[0].status).toBe("cancelled");
	});
});

describe("subscribeToJob", () => {
	it("updates active progress for the subscribed job", () => {
		const cleanup = vi.fn();
		const captured: {
			onProgress?: (update: ProgressUpdate) => void;
			onNodeDebugValue?: (event: {
				type: "node_debug_value";
				node_id: string;
				node_type: string;
				value_preview: string;
				truncated: boolean;
				preview_max_chars: number;
			}) => void;
		} = {};

		vi.spyOn(api, "subscribeToProgress").mockImplementation(
			(_jobId, progressCallback, _onClose, nodeDebugCallback) => {
				captured.onProgress = progressCallback;
				captured.onNodeDebugValue = nodeDebugCallback;
				return { close: cleanup };
			},
		);

		useJobStore.setState({
			jobs: [makeJobResponse({ id: "j1", status: "running" })],
		});

		useJobStore.getState().subscribeToJob("j1");

		const update = {
			current_frame: 10,
			total_frames: 100,
			fps: 30,
			eta_seconds: 9,
		};
		captured.onProgress?.(update);

		const state = useJobStore.getState();
		expect(state.activeJobId).toBe("j1");
		expect(state.activeProgress).toEqual(update);
		expect(state.jobs[0].progress).toEqual(update);
		expect(state.runtimePreviewsByNodeId).toEqual({});
		expect(state.wsCleanup).toBe(cleanup);
	});

	it("stores runtime preview by node id for node_debug_value events", () => {
		const cleanup = vi.fn();
		const captured: {
			onNodeDebugValue?: (event: {
				type: "node_debug_value";
				node_id: string;
				node_type: string;
				value_preview: string;
				truncated: boolean;
				preview_max_chars: number;
			}) => void;
		} = {};

		vi.spyOn(api, "subscribeToProgress").mockImplementation(
			(_jobId, _onProgress, _onClose, nodeDebugCallback) => {
				captured.onNodeDebugValue = nodeDebugCallback;
				return { close: cleanup };
			},
		);

		useJobStore.getState().subscribeToJob("j1");

		captured.onNodeDebugValue?.({
			type: "node_debug_value",
			node_id: "node-1",
			node_type: "Print",
			value_preview: "preview value",
			truncated: false,
			preview_max_chars: 512,
		});

		const preview = useJobStore.getState().runtimePreviewsByNodeId["node-1"];
		expect(preview).toMatchObject({
			node_id: "node-1",
			node_type: "Print",
			value_preview: "preview value",
			truncated: false,
			preview_max_chars: 512,
		});
		expect(preview.updated_at_ms).toBeTypeOf("number");
	});

	it("ignores stale websocket callbacks after switching jobs", async () => {
		const firstCleanup = vi.fn();
		const secondCleanup = vi.fn();
		const captured: {
			firstOnProgress?: (update: ProgressUpdate) => void;
			firstOnNodeDebugValue?: (event: {
				type: "node_debug_value";
				node_id: string;
				node_type: string;
				value_preview: string;
				truncated: boolean;
				preview_max_chars: number;
			}) => void;
			firstOnClose?: () => void;
			secondOnClose?: () => void;
		} = {};

		vi.spyOn(api, "subscribeToProgress")
			.mockImplementationOnce((_jobId, onProgress, onClose, onNodeDebugValue) => {
				captured.firstOnProgress = onProgress;
				captured.firstOnClose = onClose;
				captured.firstOnNodeDebugValue = onNodeDebugValue;
				return { close: firstCleanup };
			})
			.mockImplementationOnce((_jobId, _onProgress, onClose) => {
				captured.secondOnClose = onClose;
				return { close: secondCleanup };
			});

		vi.mocked(fetch).mockResolvedValue(
			jsonResponse([makeJobResponse({ id: "j2", status: "running" })]),
		);

		useJobStore.setState({
			jobs: [
				makeJobResponse({ id: "j1", status: "running" }),
				makeJobResponse({ id: "j2", status: "running" }),
			],
		});

		useJobStore.getState().subscribeToJob("j1");
		useJobStore.getState().subscribeToJob("j2");

		expect(firstCleanup).toHaveBeenCalledOnce();
		expect(useJobStore.getState().activeJobId).toBe("j2");

		captured.firstOnProgress?.({
			current_frame: 3,
			total_frames: 100,
			fps: 24,
			eta_seconds: 5,
		});
		captured.firstOnNodeDebugValue?.({
			type: "node_debug_value",
			node_id: "node-1",
			node_type: "Print",
			value_preview: "old",
			truncated: false,
			preview_max_chars: 512,
		});
		expect(useJobStore.getState().activeProgress).toBeNull();
		expect(
			useJobStore.getState().jobs.find((job) => job.id === "j1")?.progress,
		).toBeNull();
		expect(useJobStore.getState().runtimePreviewsByNodeId).toEqual({});

		captured.firstOnClose?.();
		expect(useJobStore.getState().activeJobId).toBe("j2");
		expect(fetch).not.toHaveBeenCalled();

		captured.secondOnClose?.();
		await vi.waitFor(() => {
			expect(fetch).toHaveBeenCalledTimes(1);
		});
		expect(useJobStore.getState().activeJobId).toBeNull();
		expect(useJobStore.getState().runtimePreviewsByNodeId).toEqual({});
		expect(useJobStore.getState().wsCleanup).toBeNull();
	});
});

describe("unsubscribeFromJob", () => {
	it("clears active job state and calls wsCleanup", () => {
		const cleanup = vi.fn();
		useJobStore.setState({
			activeJobId: "j1",
			activeProgress: {
				current_frame: 10,
				total_frames: 100,
				fps: 30,
				eta_seconds: 3,
			},
			runtimePreviewsByNodeId: {
				"node-1": {
					node_id: "node-1",
					node_type: "Print",
					value_preview: "preview",
					truncated: false,
					preview_max_chars: 512,
					updated_at_ms: 1,
				},
			},
			wsCleanup: cleanup,
		});

		useJobStore.getState().unsubscribeFromJob();

		expect(cleanup).toHaveBeenCalledOnce();
		expect(useJobStore.getState().activeJobId).toBeNull();
		expect(useJobStore.getState().activeProgress).toBeNull();
		expect(useJobStore.getState().runtimePreviewsByNodeId).toEqual({});
		expect(useJobStore.getState().wsCleanup).toBeNull();
	});

	it("is safe to call with no active subscription", () => {
		useJobStore.setState({ activeJobId: null, wsCleanup: null });
		expect(() => useJobStore.getState().unsubscribeFromJob()).not.toThrow();
	});
});

// Synthetic HTTP failures distinguish accepted mutations from failed list reads.
describe("accepted job mutations", () => {
	it.each(["submit", "run", "rerun"])("preserves the %s receipt when refresh fails", async (action) => {
		vi.mocked(fetch).mockResolvedValueOnce(jsonResponse({ id: "accepted", status: "queued", created_at: "2026-09-26T00:00:00Z" }, 201));
		vi.mocked(fetch).mockRejectedValueOnce(new TypeError("list unavailable"));
		const store = useJobStore.getState();
		const receipt = action === "submit" ? store.submitJob({ nodes: [], connections: [] })
			: action === "run" ? store.runByName("workflow") : store.rerunJob("original");
		await expect(receipt).resolves.toBe("accepted");
		expect(useJobStore.getState().jobs.some((job) => job.id === "accepted")).toBe(true);
		await vi.waitFor(() => expect(useJobStore.getState().refreshError).toBe("list unavailable"));
		expect(vi.mocked(fetch).mock.calls.filter(([, init]) => init?.method === "POST")).toHaveLength(1);
		vi.mocked(fetch).mockResolvedValueOnce(jsonResponse([makeJobResponse({ id: "accepted" })]));
		await store.fetchJobs();
		expect(useJobStore.getState().refreshError).toBeNull();
	});

	it("still rejects a failed submission without refreshing", async () => {
		vi.mocked(fetch).mockRejectedValueOnce(new TypeError("submit unavailable"));
		await expect(useJobStore.getState().submitJob({ nodes: [], connections: [] })).rejects.toThrow("submit unavailable");
		expect(fetch).toHaveBeenCalledTimes(1);
	});

	it("keeps a successful deletion when refreshing fails", async () => {
		useJobStore.setState({ jobs: [makeJobResponse()] });
		vi.mocked(fetch).mockResolvedValueOnce(new Response(null, { status: 204 }));
		vi.mocked(fetch).mockRejectedValueOnce(new TypeError("list unavailable"));
		await expect(useJobStore.getState().deleteJobHistory("j1")).resolves.toBeUndefined();
		expect(useJobStore.getState().jobs).toEqual([]);
		await vi.waitFor(() => expect(useJobStore.getState().refreshError).toBe("list unavailable"));
	});
});

it("returns a confirmed receipt before a slow refresh completes", async () => {
	let finishRefresh!: (response: Response) => void;
	vi.mocked(fetch).mockResolvedValueOnce(jsonResponse({ id: "accepted", status: "queued", created_at: "2026-09-26T00:00:00Z" }, 201));
	vi.mocked(fetch).mockReturnValueOnce(new Promise((resolve) => { finishRefresh = resolve; }));
	const id = await useJobStore.getState().submitJob({ nodes: [], connections: [] });
	expect(id).toBe("accepted");
	expect(useJobStore.getState().jobs[0]?.id).toBe("accepted");
	finishRefresh(jsonResponse([makeJobResponse({ id: "accepted", status: "running" })]));
	await vi.waitFor(() => expect(useJobStore.getState().jobs[0]?.status).toBe("running"));
});

it("does not let an older list response erase an accepted job", async () => {
	let finishOld!: (response: Response) => void;
	vi.mocked(fetch).mockReturnValueOnce(new Promise((resolve) => { finishOld = resolve; }));
	const oldRead = useJobStore.getState().fetchJobs();
	vi.mocked(fetch).mockResolvedValueOnce(jsonResponse({ id: "accepted", status: "queued", created_at: "2026-09-26T00:00:00Z" }, 201));
	vi.mocked(fetch).mockResolvedValueOnce(jsonResponse([makeJobResponse({ id: "accepted" })]));
	await useJobStore.getState().submitJob({ nodes: [], connections: [] });
	await vi.waitFor(() => expect(useJobStore.getState().jobs[0]?.status).toBe("completed"));
	finishOld(jsonResponse([]));
	await oldRead;
	expect(useJobStore.getState().jobs[0]?.id).toBe("accepted");
});

it("applies slow polling responses while a newer poll is still pending", async () => {
	let finishFirst!: (response: Response) => void;
	let finishSecond!: (response: Response) => void;
	vi.mocked(fetch).mockReturnValueOnce(new Promise((resolve) => { finishFirst = resolve; }));
	vi.mocked(fetch).mockReturnValueOnce(new Promise((resolve) => { finishSecond = resolve; }));
	const first = useJobStore.getState().fetchJobs();
	const second = useJobStore.getState().fetchJobs();
	finishFirst(jsonResponse([makeJobResponse({ status: "running" })]));
	await first;
	expect(useJobStore.getState().jobs[0]?.status).toBe("running");
	finishSecond(jsonResponse([makeJobResponse({ status: "completed" })]));
	await second;
	expect(useJobStore.getState().jobs[0]?.status).toBe("completed");
});
