import {
	applyEdgeChanges,
	applyNodeChanges,
	Background,
	BackgroundVariant,
	type Connection,
	Controls,
	type Edge,
	type EdgeChange,
	type IsValidConnection,
	MiniMap,
	type Node,
	type NodeChange,
	ReactFlow,
	ReactFlowProvider,
	useReactFlow,
} from "@xyflow/react";
import { Layers, Sparkles } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef } from "react";
import { v4 as uuidv4 } from "uuid";
import { Button } from "@/components/ui/button";
import { TooltipProvider } from "@/components/ui/tooltip";
import { useJobStore } from "@/stores/job-store";
import { useNodeDefinitions } from "@/stores/node-definitions-store";
import { useUIStore } from "@/stores/ui-store";
import { useWorkflowStore } from "@/stores/workflow-store";
import {
	type NodeTypeName,
	type PipelineNodeData,
	PORT_COLORS,
	type PortType,
} from "@/types";
import { computeLayout } from "./auto-layout";
import { CustomNode, getDefaultBackend, getPortType } from "./CustomNode";
import { DeletableEdge } from "./DeletableEdge";
import { EditorToolbar } from "./EditorToolbar";
import { NodePalette } from "./NodePalette";
import { PresetGallery } from "./PresetGallery";
import { TensorEdge } from "./TensorEdge";

const nodeTypes = { pipeline: CustomNode };
const edgeTypes = { default: DeletableEdge, tensor: TensorEdge };
const DND_MIME_PRIMARY = "application/reactflow";
const DND_MIME_FALLBACK = "text/plain";
const STARTUP_FIT_VIEW_OPTIONS = {
	padding: 0.28,
	minZoom: 0.35,
	maxZoom: 0.9,
};
const INTERACTIVE_FIT_VIEW_OPTIONS = {
	...STARTUP_FIT_VIEW_OPTIONS,
	duration: 300,
};

function readDragPayload(dataTransfer: DataTransfer, mimeType: string): string {
	try {
		return dataTransfer.getData(mimeType).trim();
	} catch {
		return "";
	}
}

function resolveDroppedNodeType(dataTransfer: DataTransfer): string {
	const primary = readDragPayload(dataTransfer, DND_MIME_PRIMARY);
	if (primary) return primary;
	return readDragPayload(dataTransfer, DND_MIME_FALLBACK);
}

function resolveEdgeStyle(portType: PortType): Record<string, string | number> {
	return {
		stroke: PORT_COLORS[portType],
		strokeWidth: 2,
	};
}

function isTensorPassthrough(
	sourceNode: Node<PipelineNodeData>,
	targetNode: Node<PipelineNodeData>,
	portType: string,
): boolean {
	if (portType !== "VideoFrames") return false;
	const srcType = sourceNode.data.nodeType as string;
	const tgtType = targetNode.data.nodeType as string;

	if (srcType === "SuperResolution" && tgtType === "FrameInterpolation") {
		const srcParams = sourceNode.data.params as Record<string, unknown>;
		const modelPath = String(srcParams.model_path ?? "");
		const tileSize = Number(srcParams.tile_size ?? 0);
		return modelPath.toLowerCase().includes("fp16") && tileSize === 0;
	}

	if (srcType === "FrameInterpolation" && tgtType === "SuperResolution") {
		return true;
	}

	return false;
}

function EditorCanvas() {
	const reactFlowWrapper = useRef<HTMLDivElement>(null);
	const { screenToFlowPosition, fitView } = useReactFlow();

	useEffect(() => {
		useNodeDefinitions.getState().fetch();
	}, []);

	const nodes = useWorkflowStore((s) => s.nodes);
	const edges = useWorkflowStore((s) => s.edges);
	const setNodes = useWorkflowStore((s) => s.setNodes);
	const setEdges = useWorkflowStore((s) => s.setEdges);
	const addNode = useWorkflowStore((s) => s.addNode);
	const addEdgeAction = useWorkflowStore((s) => s.addEdge);
	const undo = useWorkflowStore((s) => s.undo);
	const redo = useWorkflowStore((s) => s.redo);
	const jobs = useJobStore((s) => s.jobs);
	const activeJobId = useJobStore((s) => s.activeJobId);
	const fetchJobs = useJobStore((s) => s.fetchJobs);
	const subscribeToJob = useJobStore((s) => s.subscribeToJob);
	const unsubscribeFromJob = useJobStore((s) => s.unsubscribeFromJob);

	useEffect(() => {
		void fetchJobs();
		const id = setInterval(() => void fetchJobs(), 2_000);
		return () => clearInterval(id);
	}, [fetchJobs]);

	const trackedJob = useMemo(
		() =>
			jobs.find((job) => job.status === "running") ??
			jobs.find((job) => job.status === "queued") ??
			null,
		[jobs],
	);

	useEffect(() => {
		if (!trackedJob) {
			if (activeJobId !== null) {
				unsubscribeFromJob();
			}
			return;
		}

		if (activeJobId !== trackedJob.id) {
			subscribeToJob(trackedJob.id);
		}
	}, [activeJobId, subscribeToJob, trackedJob, unsubscribeFromJob]);

	useEffect(() => {
		return () => {
			useJobStore.getState().unsubscribeFromJob();
		};
	}, []);

	const onNodesChange = useCallback(
		(changes: NodeChange[]) => {
			const hasStructural = changes.some(
				(c) => c.type === "remove" || c.type === "add",
			);
			if (hasStructural) {
				setNodes(applyNodeChanges(changes, nodes) as Node<PipelineNodeData>[]);
			} else {
				useWorkflowStore.setState({
					nodes: applyNodeChanges(changes, nodes) as Node<PipelineNodeData>[],
				});
			}
		},
		[nodes, setNodes],
	);

	const onEdgesChange = useCallback(
		(changes: EdgeChange[]) => {
			const hasStructural = changes.some(
				(c) => c.type === "remove" || c.type === "add",
			);
			if (hasStructural) {
				setEdges(applyEdgeChanges(changes, edges));
			} else {
				useWorkflowStore.setState({ edges: applyEdgeChanges(changes, edges) });
			}
		},
		[edges, setEdges],
	);

	const isValidConnection: IsValidConnection = useCallback(
		(connection: Connection | Edge) => {
			if (!connection.sourceHandle || !connection.targetHandle) return false;

			const sourceNode = nodes.find((n) => n.id === connection.source);
			const targetNode = nodes.find((n) => n.id === connection.target);
			if (!sourceNode || !targetNode) return false;

			const sourcePortType = getPortType(
				sourceNode.data.nodeType as NodeTypeName,
				connection.sourceHandle,
				"output",
				sourceNode.data.params as Record<string, string | number | boolean>,
			);
			const targetPortType = getPortType(
				targetNode.data.nodeType as NodeTypeName,
				connection.targetHandle,
				"input",
				targetNode.data.params as Record<string, string | number | boolean>,
			);

			return sourcePortType !== undefined && sourcePortType === targetPortType;
		},
		[nodes],
	);

	const onConnect = useCallback(
		(connection: Connection) => {
			if (!connection.sourceHandle || !connection.targetHandle) return;

			const sourceNode = nodes.find((n) => n.id === connection.source);
			if (!sourceNode) return;

			const portType =
				getPortType(
					sourceNode.data.nodeType as NodeTypeName,
					connection.sourceHandle,
					"output",
					sourceNode.data.params as Record<string, string | number | boolean>,
				) ?? "Str";

			const targetNode = nodes.find((n) => n.id === connection.target);
			const tensor =
				sourceNode && targetNode
					? isTensorPassthrough(sourceNode, targetNode, portType)
					: false;

			const edge: Edge = {
				id: `e-${connection.source}-${connection.sourceHandle}-${connection.target}-${connection.targetHandle}`,
				source: connection.source,
				sourceHandle: connection.sourceHandle,
				target: connection.target,
				targetHandle: connection.targetHandle,
				...(tensor
					? { type: "tensor" }
					: {
							style: resolveEdgeStyle(portType),
							animated: portType === "VideoFrames",
						}),
				data: { port_type: portType },
			};

			addEdgeAction(edge);
		},
		[nodes, addEdgeAction],
	);

	const onDragOver = useCallback((event: React.DragEvent) => {
		event.preventDefault();
		event.dataTransfer.dropEffect = "move";
	}, []);

	const onDrop = useCallback(
		(event: React.DragEvent) => {
			event.preventDefault();
			const nodeType = resolveDroppedNodeType(event.dataTransfer);
			if (!nodeType) return;

			const descriptors = useNodeDefinitions.getState().descriptors;
			const desc = descriptors.find((d) => d.node_type === nodeType);
			if (!desc) return;

			const position = screenToFlowPosition({
				x: event.clientX,
				y: event.clientY,
			});

			const defaultParams: Record<string, string | number | boolean> = {};
			for (const port of desc.inputs) {
				if (
					port.direction === "param" &&
					port.default_value !== null &&
					port.default_value !== undefined
				) {
					if (port.name === "backend") {
						defaultParams[port.name] = getDefaultBackend();
					} else {
						defaultParams[port.name] = port.default_value as
							| string
							| number
							| boolean;
					}
				}
			}

			addNode({
				id: uuidv4(),
				type: "pipeline",
				position,
				data: { nodeType, params: defaultParams } satisfies PipelineNodeData,
			});
		},
		[screenToFlowPosition, addNode],
	);

	useEffect(() => {
		function handleKeyDown(e: KeyboardEvent) {
			if ((e.ctrlKey || e.metaKey) && e.key === "z" && !e.shiftKey) {
				e.preventDefault();
				undo();
			}
			if ((e.ctrlKey || e.metaKey) && e.key === "Z" && e.shiftKey) {
				e.preventDefault();
				redo();
			}
			if (e.key === "Escape") {
				useUIStore.getState().closeModal();
			}
		}
		window.addEventListener("keydown", handleKeyDown);
		return () => {
			window.removeEventListener("keydown", handleKeyDown);
		};
	}, [undo, redo]);

	const handleAutoLayout = useCallback(() => {
		const positions = computeLayout(nodes, edges);
		const updatedNodes = nodes.map((n) => ({
			...n,
			position: positions[n.id] ?? n.position,
		}));
		setNodes(updatedNodes);
		setTimeout(() => {
			fitView(INTERACTIVE_FIT_VIEW_OPTIONS);
		}, 50);
	}, [nodes, edges, setNodes, fitView]);

	const handleFitView = useCallback(() => {
		fitView(INTERACTIVE_FIT_VIEW_OPTIONS);
	}, [fitView]);

	const styledEdges = useMemo(
		() =>
			edges.map((edge) => {
				if (edge.type === "tensor") return edge;
				const portType =
					edge.data && typeof edge.data === "object" && "port_type" in edge.data
						? (edge.data.port_type as PortType)
						: "Str";

				const srcNode = nodes.find((n) => n.id === edge.source);
				const tgtNode = nodes.find((n) => n.id === edge.target);
				if (
					srcNode &&
					tgtNode &&
					isTensorPassthrough(srcNode, tgtNode, portType)
				) {
					return { ...edge, type: "tensor" as const };
				}

				return {
					...edge,
					style: resolveEdgeStyle(portType),
					animated: portType === "VideoFrames",
				};
			}),
		[edges, nodes],
	);

	const miniMapNodeColor = useCallback(
		(n: { data: Record<string, unknown> }) => {
			const nt = (n.data as PipelineNodeData).nodeType;
			const descriptors = useNodeDefinitions.getState().descriptors;
			const desc = descriptors.find((d) => d.node_type === nt);
			return desc?.accent_color ?? "#888";
		},
		[],
	);

	return (
		<div className="flex h-full w-full">
			<NodePalette />
			<div className="relative flex-1" ref={reactFlowWrapper}>
				<EditorToolbar
					onAutoLayout={handleAutoLayout}
					onFitView={handleFitView}
				/>
				<ReactFlow
					nodes={nodes}
					edges={styledEdges}
					onNodesChange={onNodesChange}
					onEdgesChange={onEdgesChange}
					onConnect={onConnect}
					onDrop={onDrop}
					onDragOver={onDragOver}
					isValidConnection={isValidConnection}
					nodeTypes={nodeTypes}
					edgeTypes={edgeTypes}
					fitView
					fitViewOptions={STARTUP_FIT_VIEW_OPTIONS}
					deleteKeyCode="Delete"
					connectionLineStyle={{ stroke: "var(--ring)", strokeWidth: 2 }}
					defaultEdgeOptions={{ type: "default" }}
					proOptions={{ hideAttribution: true }}
				>
					<svg
						style={{ position: "absolute", width: 0, height: 0 }}
						aria-hidden="true"
					>
						<defs>
							<linearGradient
								id="tensor-edge-aura"
								x1="0%"
								y1="0%"
								x2="100%"
								y2="0%"
							>
								<stop offset="0%" stopColor="#22D3EE" />
								<stop offset="100%" stopColor="#A78BFA" />
							</linearGradient>
							<linearGradient
								id="tensor-edge-base"
								x1="0%"
								y1="0%"
								x2="100%"
								y2="0%"
							>
								<stop offset="0%" stopColor="#0284C7" />
								<stop offset="45%" stopColor="#06B6D4" />
								<stop offset="100%" stopColor="#7C3AED" />
							</linearGradient>
							<linearGradient
								id="tensor-edge-lane"
								x1="0%"
								y1="0%"
								x2="100%"
								y2="0%"
							>
								<stop offset="0%" stopColor="#ECFEFF" />
								<stop offset="50%" stopColor="#67E8F9" />
								<stop offset="100%" stopColor="#DDD6FE" />
							</linearGradient>
						</defs>
					</svg>
					<Background
						variant={BackgroundVariant.Dots}
						gap={20}
						size={1}
						color="var(--border)"
					/>
					<MiniMap
						className="!bg-card/80 !border-border/50"
						nodeColor={miniMapNodeColor}
						maskColor="rgba(0,0,0,0.5)"
					/>
					<Controls className="!bg-card/90 !border-border/50 !shadow-lg" />
				</ReactFlow>
				{nodes.length === 0 && (
					<div className="pointer-events-none absolute inset-0 flex items-center justify-center z-[5]">
						<div className="pointer-events-auto flex flex-col items-center gap-4 text-center">
							<div className="flex h-16 w-16 items-center justify-center rounded-2xl bg-secondary/60">
								<Layers className="h-7 w-7 text-muted-foreground" />
							</div>
							<div>
								<p className="text-sm font-medium text-foreground/80">
									Drag nodes from the sidebar to build a pipeline
								</p>
								<p className="text-xs text-muted-foreground mt-1">
									Or load a preset to get started
								</p>
							</div>
							<Button
								variant="secondary"
								size="sm"
								onClick={() => {
									useUIStore.getState().openModal("presets");
								}}
							>
								<Sparkles className="h-3.5 w-3.5" />
								Load a Preset
							</Button>
						</div>
					</div>
				)}
				<PresetGallery />
			</div>
		</div>
	);
}

export function EditorPage() {
	return (
		<ReactFlowProvider>
			<TooltipProvider delayDuration={200}>
				<EditorCanvas />
			</TooltipProvider>
		</ReactFlowProvider>
	);
}
