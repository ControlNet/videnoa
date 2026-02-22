import { create } from "zustand";
import * as api from "../api/client";
import type { Job, NodeRuntimePreview, ProgressUpdate, Workflow } from "../types";

interface JobState {
	jobs: Job[];
	activeJobId: string | null;
	activeProgress: ProgressUpdate | null;
	runtimePreviewsByNodeId: Record<string, NodeRuntimePreview>;
	runtimePreviewsByJobId: Record<string, Record<string, NodeRuntimePreview>>;
	wsCleanup: (() => void) | null;

	fetchJobs: () => Promise<void>;
	submitJob: (
		workflow: Workflow,
		options?: { workflowName?: string },
	) => Promise<string>;
	runByName: (
		workflowName: string,
		params?: Record<string, string | number | boolean>,
	) => Promise<string>;
	rerunJob: (jobId: string) => Promise<string>;
	deleteJobHistory: (jobId: string) => Promise<void>;
	cancelJob: (jobId: string) => Promise<void>;
	subscribeToJob: (jobId: string) => void;
	unsubscribeFromJob: () => void;
}

export const useJobStore = create<JobState>((set, get) => ({
	jobs: [],
	activeJobId: null,
	activeProgress: null,
	runtimePreviewsByNodeId: {},
	runtimePreviewsByJobId: {},
	wsCleanup: null,

	fetchJobs: async () => {
		const responses = await api.listJobs();
		const jobs: Job[] = responses.map((r) => ({
			id: r.id,
			status: r.status,
			progress: r.progress,
			created_at: r.created_at,
			started_at: r.started_at,
			completed_at: r.completed_at,
			error: r.error,
			workflow_name: r.workflow_name,
			workflow_source: r.workflow_source,
			params: r.params,
			rerun_of_job_id: r.rerun_of_job_id,
			duration_ms: r.duration_ms,
		}));
		set({ jobs });
	},

	submitJob: async (workflow, options) => {
		const response = await api.submitJob(workflow, options);
		await get().fetchJobs();
		return response.id;
	},

	runByName: async (workflowName, params) => {
		const response = await api.runByName(workflowName, params);
		await get().fetchJobs();
		return response.id;
	},

	rerunJob: async (jobId) => {
		const response = await api.rerunJob(jobId);
		await get().fetchJobs();
		return response.id;
	},

	deleteJobHistory: async (jobId) => {
		await api.deleteJobHistory(jobId);
		await get().fetchJobs();
	},

	cancelJob: async (jobId) => {
		await get().deleteJobHistory(jobId);
	},

	subscribeToJob: (jobId) => {
		const state = get();
		if (state.activeJobId === jobId && state.wsCleanup) {
			return;
		}

		state.wsCleanup?.();

		const { close } = api.subscribeToProgress(
			jobId,
			(update) => {
				if (get().activeJobId !== jobId) {
					return;
				}

				set((prev) => ({
					activeProgress: update,
					jobs: prev.jobs.map((job) =>
						job.id === jobId ? { ...job, progress: update } : job,
					),
				}));
			},
			() => {
				if (get().activeJobId !== jobId) {
					return;
				}

				set({
					activeJobId: null,
					activeProgress: null,
					runtimePreviewsByNodeId: {},
					wsCleanup: null,
				});
				get()
					.fetchJobs()
					.catch((err: unknown) => {
						console.error("Failed to refresh jobs after WS close:", err);
					});
			},
			(event) => {
				if (get().activeJobId !== jobId) {
					return;
				}

				const runtimePreview: NodeRuntimePreview = {
					...event,
					updated_at_ms: Date.now(),
				};

				set((prev) => ({
					runtimePreviewsByNodeId: {
						...prev.runtimePreviewsByNodeId,
						[event.node_id]: runtimePreview,
					},
					runtimePreviewsByJobId: {
						...prev.runtimePreviewsByJobId,
						[jobId]: {
							...(prev.runtimePreviewsByJobId[jobId] ?? {}),
							[event.node_id]: runtimePreview,
						},
					},
				}));
			},
		);

		set({
			activeJobId: jobId,
			activeProgress: null,
			runtimePreviewsByNodeId: {},
			wsCleanup: close,
		});
	},

	unsubscribeFromJob: () => {
		const { wsCleanup } = get();
		set({
			activeJobId: null,
			activeProgress: null,
			runtimePreviewsByNodeId: {},
			wsCleanup: null,
		});
		wsCleanup?.();
	},
}));
