import { act, render } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useJobStore } from "@/stores/job-store";
import { useNodeDefinitions } from "@/stores/node-definitions-store";
import { useWorkflowStore } from "@/stores/workflow-store";
import { EditorPage } from "../EditorPage";

const reactFlowMock = vi.hoisted(() => {
	Object.defineProperty(window, "matchMedia", {
		writable: true,
		value: vi.fn().mockImplementation((query: string) => ({
			matches: query === "(prefers-color-scheme: dark)",
			media: query,
			onchange: null,
			addListener: vi.fn(),
			removeListener: vi.fn(),
			addEventListener: vi.fn(),
			removeEventListener: vi.fn(),
			dispatchEvent: vi.fn(),
		})),
	});

	return {
		props: {} as Record<string, unknown>,
		screenToFlowPosition: vi.fn(({ x, y }: { x: number; y: number }) => ({
			x,
			y,
		})),
		fitView: vi.fn(),
	};
});

const jobStoreMocks = vi.hoisted(() => ({
	fetchJobs: vi.fn().mockResolvedValue(undefined),
	subscribeToJob: vi.fn(),
	unsubscribeFromJob: vi.fn(),
}));

vi.mock("@xyflow/react", () => ({
	ReactFlow: (props: Record<string, unknown>) => {
		reactFlowMock.props = props;
		return (
			<div data-testid="react-flow">{props.children as React.ReactNode}</div>
		);
	},
	ReactFlowProvider: ({ children }: { children: React.ReactNode }) => (
		<>{children}</>
	),
	Background: () => null,
	MiniMap: () => null,
	Controls: () => null,
	BackgroundVariant: { Dots: "dots" },
	useReactFlow: () => ({
		screenToFlowPosition: reactFlowMock.screenToFlowPosition,
		fitView: reactFlowMock.fitView,
	}),
	applyNodeChanges: (_changes: unknown, nodes: unknown) => nodes,
	applyEdgeChanges: (_changes: unknown, edges: unknown) => edges,
}));

vi.mock("../NodePalette", () => ({
	NodePalette: () => null,
}));

vi.mock("../EditorToolbar", () => ({
	EditorToolbar: () => null,
}));

vi.mock("../PresetGallery", () => ({
	PresetGallery: () => null,
}));

vi.mock("../CustomNode", () => ({
	CustomNode: () => null,
	getPortType: vi.fn(
		(
			nodeType: string,
			handleId: string,
			direction: "input" | "output",
			nodeParams?: Record<string, string | number | boolean>,
		) => {
			if (
				nodeType === "TypeConversion" &&
				handleId === "value" &&
				direction === "output"
			) {
				const outputType = nodeParams?.output_type;
				return outputType === "Bool" ? "Bool" : "Str";
			}
			if (
				nodeType === "VideoOutput" &&
				handleId === "codec" &&
				direction === "input"
			) {
				return "Str";
			}
			if (
				nodeType === "VideoOutput" &&
				handleId === "crf" &&
				direction === "input"
			) {
				return "Int";
			}
			return "VideoFrames";
		},
	),
	getDefaultBackend: () => "cuda",
}));

vi.mock("../DeletableEdge", () => ({
	DeletableEdge: () => null,
}));

vi.mock("../TensorEdge", () => ({
	TensorEdge: () => null,
}));

vi.mock("../auto-layout", () => ({
	computeLayout: vi.fn(() => ({})),
}));

const VIDEO_INPUT_DESCRIPTOR = {
	node_type: "VideoInput",
	display_name: "Video Input",
	category: "input",
	accent_color: "#8B5CF6",
	icon: "file-video",
	inputs: [
		{
			name: "path",
			port_type: "Path",
			direction: "param",
			required: true,
			default_value: null,
			ui_hint: null,
			enum_options: null,
			dynamic_type_param: null,
		},
	],
	outputs: [
		{
			name: "frames",
			port_type: "VideoFrames",
			direction: "stream",
			required: true,
			default_value: null,
			ui_hint: null,
			enum_options: null,
			dynamic_type_param: null,
		},
	],
};

function getOnDropHandler() {
	const onDrop = reactFlowMock.props.onDrop;
	expect(onDrop).toBeTypeOf("function");
	return onDrop as (event: React.DragEvent) => void;
}

beforeEach(() => {
	vi.clearAllMocks();
	reactFlowMock.props = {};
	reactFlowMock.screenToFlowPosition.mockImplementation(
		({ x, y }: { x: number; y: number }) => ({ x, y }),
	);

	useWorkflowStore.setState({
		nodes: [],
		edges: [],
		past: [],
		future: [],
		currentFile: null,
	});

	jobStoreMocks.fetchJobs.mockResolvedValue(undefined);
	useJobStore.setState({
		jobs: [],
		activeJobId: null,
		activeProgress: null,
		runtimePreviewsByNodeId: {},
		wsCleanup: null,
		fetchJobs: jobStoreMocks.fetchJobs,
		subscribeToJob: jobStoreMocks.subscribeToJob,
		unsubscribeFromJob: jobStoreMocks.unsubscribeFromJob,
	});

	useNodeDefinitions.setState({
		descriptors: [VIDEO_INPUT_DESCRIPTOR],
		loading: false,
		error: null,
		fetch: vi.fn().mockResolvedValue(undefined),
	});
});

describe("EditorPage drag-and-drop", () => {
	it("uses capped startup fit-view options for initial readability", () => {
		render(<EditorPage />);

		expect(reactFlowMock.props.fitView).toBe(true);
		expect(reactFlowMock.props.fitViewOptions).toMatchObject({
			padding: 0.28,
			minZoom: 0.35,
			maxZoom: 0.9,
		});
	});

	it("uses application/reactflow payload when available", () => {
		render(<EditorPage />);
		const onDrop = getOnDropHandler();

		const getData = vi.fn((mimeType: string) => {
			if (mimeType === "application/reactflow") return "VideoInput";
			return "UnknownFromFallback";
		});

		act(() => {
			onDrop({
				preventDefault: vi.fn(),
				clientX: 300,
				clientY: 180,
				dataTransfer: { getData } as unknown as DataTransfer,
			} as unknown as React.DragEvent);
		});

		expect(getData).toHaveBeenCalledWith("application/reactflow");
		expect(useWorkflowStore.getState().nodes).toHaveLength(1);
		expect(useWorkflowStore.getState().nodes[0].data.nodeType).toBe(
			"VideoInput",
		);
	});

	it("falls back to text/plain payload when custom MIME is empty", () => {
		render(<EditorPage />);
		const onDrop = getOnDropHandler();

		const getData = vi.fn((mimeType: string) => {
			if (mimeType === "application/reactflow") return "";
			if (mimeType === "text/plain") return "VideoInput";
			return "";
		});

		act(() => {
			onDrop({
				preventDefault: vi.fn(),
				clientX: 120,
				clientY: 44,
				dataTransfer: { getData } as unknown as DataTransfer,
			} as unknown as React.DragEvent);
		});

		expect(getData).toHaveBeenCalledWith("application/reactflow");
		expect(getData).toHaveBeenCalledWith("text/plain");
		expect(useWorkflowStore.getState().nodes).toHaveLength(1);
		expect(useWorkflowStore.getState().nodes[0].data.nodeType).toBe(
			"VideoInput",
		);
	});

	it("safely no-ops for invalid payload values", () => {
		render(<EditorPage />);
		const onDrop = getOnDropHandler();

		const getData = vi.fn((mimeType: string) => {
			if (mimeType === "application/reactflow") return "";
			if (mimeType === "text/plain") return "UnknownNodeType";
			return "";
		});

		act(() => {
			onDrop({
				preventDefault: vi.fn(),
				clientX: 90,
				clientY: 30,
				dataTransfer: { getData } as unknown as DataTransfer,
			} as unknown as React.DragEvent);
		});

		expect(useWorkflowStore.getState().nodes).toHaveLength(0);
	});

	it("enforces dynamic output type in isValidConnection", () => {
		const makeNodes = (
			outputType: "Str" | "Bool",
		): Array<{
			id: string;
			type: string;
			position: { x: number; y: number };
			data: {
				nodeType: string;
				params: Record<string, string | number | boolean>;
			};
		}> => [
			{
				id: "conv",
				type: "pipeline",
				position: { x: 0, y: 0 },
				data: {
					nodeType: "TypeConversion",
					params: { input_type: "Str", output_type: outputType },
				},
			},
			{
				id: "out",
				type: "pipeline",
				position: { x: 120, y: 0 },
				data: { nodeType: "VideoOutput", params: { codec: "libx265" } },
			},
		];

		useWorkflowStore.setState({
			nodes: makeNodes("Str"),
			edges: [],
		});

		const firstRender = render(<EditorPage />);
		const firstIsValidConnection = reactFlowMock.props.isValidConnection as
			| ((connection: {
					source: string;
					sourceHandle: string;
					target: string;
					targetHandle: string;
			  }) => boolean)
			| undefined;
		expect(firstIsValidConnection).toBeTypeOf("function");

		const validWhenStr = firstIsValidConnection?.({
			source: "conv",
			sourceHandle: "value",
			target: "out",
			targetHandle: "codec",
		});
		expect(validWhenStr).toBe(true);
		firstRender.unmount();

		useWorkflowStore.setState({
			nodes: makeNodes("Bool"),
			edges: [],
		});

		render(<EditorPage />);
		const secondIsValidConnection = reactFlowMock.props.isValidConnection as
			| ((connection: {
					source: string;
					sourceHandle: string;
					target: string;
					targetHandle: string;
			  }) => boolean)
			| undefined;
		expect(secondIsValidConnection).toBeTypeOf("function");

		const invalidWhenBool = secondIsValidConnection?.({
			source: "conv",
			sourceHandle: "value",
			target: "out",
			targetHandle: "codec",
		});

		expect(invalidWhenBool).toBe(false);
	});

	it("subscribes to running jobs and cleans up on unmount", () => {
		useJobStore.setState({
			jobs: [
				{
					id: "job-1",
					status: "running",
					progress: null,
					created_at: "2026-02-14T00:00:00Z",
					started_at: null,
					completed_at: null,
					error: null,
				},
			],
		});

		const view = render(<EditorPage />);

		expect(jobStoreMocks.fetchJobs).toHaveBeenCalledTimes(1);
		expect(jobStoreMocks.subscribeToJob).toHaveBeenCalledWith("job-1");

		view.unmount();
		expect(jobStoreMocks.unsubscribeFromJob).toHaveBeenCalled();
	});

	it("unsubscribes when no tracked job exists but active job id is set", () => {
		useJobStore.setState({ activeJobId: "job-stale" });

		render(<EditorPage />);

		expect(jobStoreMocks.unsubscribeFromJob).toHaveBeenCalled();
	});
});
