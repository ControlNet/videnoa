import dagre from "@dagrejs/dagre";
import type { Edge, Node } from "@xyflow/react";
import {
	Background,
	BackgroundVariant,
	Controls,
	getNodesBounds,
	Handle,
	MiniMap,
	Position,
	ReactFlow,
	type ReactFlowInstance,
	ReactFlowProvider,
} from "@xyflow/react";
import { Loader2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { GraphNodeInfo, ModelEntry, ModelInspection } from "@/api/client";
import { inspectModel } from "@/api/client";
import { Badge } from "@/components/ui/badge";
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogHeader,
	DialogTitle,
} from "@/components/ui/dialog";
import {
	formatErrorWithPrefix,
	getErrorMessage,
} from "@/lib/presentation-error";
import { formatCompactNumber } from "@/lib/presentation-format";
import { cn } from "@/lib/utils";

const GRAPH_NODE_W = 180;
const GRAPH_NODE_H = 48;

/*
 * Weights are not architecture. A Constant node carries a tensor a Conv needs,
 * and on the smallest model shipped here they are 32 of the 55 nodes -- drawing
 * them buries the 23 that describe what the model does.
 */
const CONSTANT_OP = "Constant";

/* Comfortably past every model shipped here (RIFE is the largest at 462 after
 * Constants are dropped), while still refusing to lay out something pathological. */
const MAX_GRAPH_NODES = 1500;

/* The smallest zoom at which a node's label is still worth reading. */
const READABLE_ZOOM = 0.8;

function formatShape(shape: number[]): string {
	const dims = shape.map((d) => (d < 0 ? "?" : String(d)));
	return `[${dims.join(", ")}]`;
}

function structuralNodes(inspection: ModelInspection): GraphNodeInfo[] {
	return inspection.nodes.filter((node) => node.op_type !== CONSTANT_OP);
}

// ─── Graph layout ─────────────────────────────────────────────────────────────

interface OnnxGraphData {
	nodes: Node[];
	edges: Edge[];
}

function buildOnnxGraph(inspection: ModelInspection): OnnxGraphData {
	const rfNodes: Node[] = [];
	const rfEdges: Edge[] = [];

	const nodes = structuralNodes(inspection);
	const outputToNodeId = new Map<string, string>();
	const graphOutputNames = new Set(inspection.outputs.map((t) => t.name));

	for (let i = 0; i < nodes.length; i++) {
		const node = nodes[i];
		const nodeId = `op-${String(i)}`;
		rfNodes.push({
			id: nodeId,
			position: { x: 0, y: 0 },
			data: { label: node.op_type, subtitle: node.name },
			type: "onnxOp",
		});
		for (const out of node.outputs) {
			outputToNodeId.set(out, nodeId);
		}
	}

	// Initializers (weights/biases) have fully static shapes; a real graph input
	// carries a dynamic dimension. Keep the inputs, drop the parameter tensors.
	const isLargeModel = nodes.length > 50;
	for (const inp of inspection.inputs) {
		const hasDynamicDim = inp.shape.some((d) => d <= 0);
		if (isLargeModel && !hasDynamicDim && inp.shape.length > 0) {
			continue;
		}
		const inputNodeId = `input-${inp.name}`;
		rfNodes.push({
			id: inputNodeId,
			position: { x: 0, y: 0 },
			data: {
				label: inp.name,
				subtitle: `${inp.data_type} ${formatShape(inp.shape)}`,
			},
			type: "onnxIO",
		});
		outputToNodeId.set(inp.name, inputNodeId);
	}

	for (const out of inspection.outputs) {
		rfNodes.push({
			id: `output-${out.name}`,
			position: { x: 0, y: 0 },
			data: {
				label: out.name,
				subtitle: `${out.data_type} ${formatShape(out.shape)}`,
			},
			type: "onnxIO",
		});
	}

	let edgeIdx = 0;
	for (let i = 0; i < nodes.length; i++) {
		const node = nodes[i];
		const targetId = `op-${String(i)}`;
		for (const inputTensor of node.inputs) {
			if (!inputTensor) continue;
			const sourceId = outputToNodeId.get(inputTensor);
			if (sourceId) {
				rfEdges.push({
					id: `e-${String(edgeIdx++)}`,
					source: sourceId,
					target: targetId,
				});
			}
		}

		for (const outputTensor of node.outputs) {
			if (graphOutputNames.has(outputTensor)) {
				rfEdges.push({
					id: `e-${String(edgeIdx++)}`,
					source: targetId,
					target: `output-${outputTensor}`,
				});
			}
		}
	}

	// Left to right, like the pipeline graph in the Editor.
	const g = new dagre.graphlib.Graph();
	g.setGraph({ rankdir: "LR", nodesep: 24, ranksep: 56 });
	g.setDefaultEdgeLabel(() => ({}));

	for (const n of rfNodes) {
		g.setNode(n.id, { width: GRAPH_NODE_W, height: GRAPH_NODE_H });
	}
	for (const e of rfEdges) {
		g.setEdge(e.source, e.target);
	}

	dagre.layout(g);

	const positioned = rfNodes.map((n) => {
		const pos = g.node(n.id);
		return {
			...n,
			position: pos
				? { x: pos.x - GRAPH_NODE_W / 2, y: pos.y - GRAPH_NODE_H / 2 }
				: n.position,
		};
	});

	return { nodes: positioned, edges: rfEdges };
}

// ─── Graph nodes ──────────────────────────────────────────────────────────────

/*
 * The handles are what React Flow anchors an edge to. Without them every edge
 * is dropped, which is why this graph used to render as disconnected boxes.
 */
function NodeHandles() {
	return (
		<>
			<Handle
				type="target"
				position={Position.Left}
				className="!size-1.5 !border-0 !bg-muted-foreground/60"
			/>
			<Handle
				type="source"
				position={Position.Right}
				className="!size-1.5 !border-0 !bg-muted-foreground/60"
			/>
		</>
	);
}

function OnnxOpNode({ data }: { data: { label: string; subtitle: string } }) {
	return (
		<div className="w-[180px] rounded-lg border border-border bg-card px-3 py-2 shadow-sm">
			<NodeHandles />
			<div className="truncate text-[13px] font-semibold leading-[18px]">
				{data.label}
			</div>
			{data.subtitle && (
				<div className="truncate font-mono text-[11px] leading-4 text-muted-foreground">
					{data.subtitle}
				</div>
			)}
		</div>
	);
}

function OnnxIONode({ data }: { data: { label: string; subtitle: string } }) {
	return (
		<div className="tone-badge tone-io w-[180px] rounded-lg border px-3 py-2">
			<NodeHandles />
			<div className="truncate font-mono text-[13px] font-semibold leading-[18px]">
				{data.label}
			</div>
			{data.subtitle && (
				<div className="truncate font-mono text-[11px] leading-4 text-muted-foreground">
					{data.subtitle}
				</div>
			)}
		</div>
	);
}

const onnxNodeTypes = {
	onnxOp: OnnxOpNode,
	onnxIO: OnnxIONode,
};

// ─── Architecture view ────────────────────────────────────────────────────────

function GraphSection({ inspection }: { inspection: ModelInspection }) {
	const { t } = useTranslation("models");
	const structuralCount = structuralNodes(inspection).length;
	const tooLarge = structuralCount > MAX_GRAPH_NODES;

	const { nodes, edges } = useMemo(
		() => (tooLarge ? { nodes: [], edges: [] } : buildOnnxGraph(inspection)),
		[inspection, tooLarge],
	);

	const wrapperRef = useRef<HTMLDivElement>(null);

	const miniMapNodeColor = useCallback(
		(n: { type?: string }) => (n.type === "onnxIO" ? "#10b981" : "#8b8b95"),
		[],
	);

	/*
	 * A long chain cannot be both whole and legible in one viewport. Fit it when
	 * that is possible, and otherwise open at the model's input at a readable
	 * zoom -- panning to the rest costs a scroll, and the fit control still
	 * reaches the whole graph because the zoom floor no longer stops at 0.5.
	 */
	const handleInit = useCallback(
		(instance: ReactFlowInstance) => {
			const element = wrapperRef.current;
			if (!element || nodes.length === 0) {
				void instance.fitView({ padding: 0.12 });
				return;
			}

			const { width, height } = element.getBoundingClientRect();
			const bounds = getNodesBounds(nodes);
			const fitZoom = Math.min(
				width / (bounds.width * 1.12),
				height / (bounds.height * 1.12),
			);

			if (fitZoom >= READABLE_ZOOM) {
				void instance.fitView({ padding: 0.12 });
				return;
			}

			void instance.setViewport({
				x: 32 - bounds.x * READABLE_ZOOM,
				y: height / 2 - (bounds.y + bounds.height / 2) * READABLE_ZOOM,
				zoom: READABLE_ZOOM,
			});
		},
		[nodes],
	);

	if (tooLarge) {
		return (
			<div className="flex h-full items-center justify-center bg-background">
				<div className="space-y-2 text-center">
					<p className="text-sm text-muted-foreground">
						{t("detail.graph.tooLarge", { count: structuralCount })}
					</p>
					<p className="text-xs text-muted-foreground">
						{t("detail.graph.tooLargeHint")}
					</p>
				</div>
			</div>
		);
	}

	return (
		<div ref={wrapperRef} className="relative h-full bg-background">
			<ReactFlow
				nodes={nodes}
				edges={edges}
				nodeTypes={onnxNodeTypes}
				onInit={handleInit}
				/* The default floor of 0.5 could not fit a graph this wide, so
				   "fit view" never actually fit the view. */
				minZoom={0.03}
				nodesDraggable={false}
				nodesConnectable={false}
				elementsSelectable={false}
				panOnScroll
				zoomOnScroll
				proOptions={{ hideAttribution: true }}
				defaultEdgeOptions={{
					type: "smoothstep",
					style: { stroke: "var(--border)", strokeWidth: 1.5 },
					animated: false,
				}}
			>
				<Background
					variant={BackgroundVariant.Dots}
					gap={16}
					size={1}
					color="var(--border)"
				/>
				<MiniMap
					className="!bg-card/80 !border-border/50"
					nodeColor={miniMapNodeColor}
					/* A flat black mask is invisible in dark and a grey slab in light. */
					maskColor="color-mix(in oklab, var(--background) 68%, transparent)"
					pannable
					zoomable
				/>
				<Controls className="!bg-card !border-border/50 !shadow-sm" />
			</ReactFlow>
		</div>
	);
}

// ─── Operators view ───────────────────────────────────────────────────────────

function OperatorsSection({ inspection }: { inspection: ModelInspection }) {
	const { t } = useTranslation("models");
	const sequence = structuralNodes(inspection);

	const byType = useMemo(() => {
		const counts = new Map<string, number>();
		for (const node of inspection.nodes) {
			counts.set(node.op_type, (counts.get(node.op_type) ?? 0) + 1);
		}
		return [...counts.entries()].sort((a, b) => b[1] - a[1]);
	}, [inspection]);

	const maxCount = byType[0]?.[1] ?? 1;

	return (
		<div className="grid h-full grid-cols-1 gap-8 overflow-y-auto px-7 py-6 lg:grid-cols-2">
			<div>
				<h4 className="text-[11px] font-semibold uppercase tracking-[0.08em] text-muted-foreground">
					{t("detail.operators.byType", { total: inspection.nodes.length })}
				</h4>
				<div className="mt-2.5 mb-3.5 h-px bg-border" />
				<div className="flex flex-col gap-2.5">
					{byType.map(([opType, count]) => (
						<div key={opType} className="flex items-center gap-3">
							<span className="w-28 shrink-0 truncate font-mono text-xs">
								{opType}
							</span>
							<span className="h-1.5 flex-1 overflow-hidden rounded-full bg-foreground/[0.08]">
								<span
									className={cn(
										"block h-full rounded-full",
										opType === CONSTANT_OP ? "bg-foreground/25" : "bg-primary",
									)}
									style={{ width: `${String((count / maxCount) * 100)}%` }}
								/>
							</span>
							<span className="w-7 shrink-0 text-right font-mono text-xs text-muted-foreground">
								{count}
							</span>
						</div>
					))}
				</div>
			</div>

			<div className="min-w-0">
				<h4 className="text-[11px] font-semibold uppercase tracking-[0.08em] text-muted-foreground">
					{t("detail.operators.sequence", { total: sequence.length })}
				</h4>
				<div className="mt-2.5 mb-2.5 h-px bg-border" />
				<ol className="flex flex-col">
					{sequence.map((node, index) => (
						<li
							key={`${node.name}-${String(index)}`}
							className="flex h-[22px] items-center gap-2.5"
						>
							<span className="w-6 shrink-0 text-right font-mono text-[11px] text-muted-foreground/75">
								{index + 1}
							</span>
							<span className="w-24 shrink-0 truncate text-xs font-medium">
								{node.op_type}
							</span>
							<span className="min-w-0 flex-1 truncate font-mono text-[11px] text-muted-foreground">
								{node.name}
							</span>
						</li>
					))}
				</ol>
			</div>
		</div>
	);
}

// ─── Rail ─────────────────────────────────────────────────────────────────────

function RailSection({
	title,
	children,
}: {
	title: string;
	children: React.ReactNode;
}) {
	return (
		<section className="mb-5">
			<h4 className="text-[11px] font-semibold uppercase tracking-[0.08em] text-muted-foreground">
				{title}
			</h4>
			<div className="mt-2 mb-3 h-px bg-border" />
			{children}
		</section>
	);
}

function TensorList({
	tensors,
}: {
	tensors: ModelInspection["inputs"];
}) {
	return (
		<div className="flex flex-col gap-3">
			{tensors.map((tensor) => (
				<div key={tensor.name}>
					<div className="truncate font-mono text-xs font-medium">
						{tensor.name}
					</div>
					<div className="mt-1 flex items-center gap-2">
						<Badge
							variant="outline"
							className="tone-badge tone-spec px-1.5 py-0 font-mono text-[10px] font-medium"
						>
							{tensor.data_type}
						</Badge>
						<span className="font-mono text-[11px] text-muted-foreground">
							{formatShape(tensor.shape)}
						</span>
					</div>
				</div>
			))}
		</div>
	);
}

/* Metadata and the schema are the graph in another form, so they sit beside it
 * rather than behind a tab that hides one to show the other. */
function DetailRail({
	model,
	inspection,
}: {
	model: ModelEntry;
	inspection: ModelInspection;
}) {
	const { t, i18n } = useTranslation(["models", "common"]);
	const producerValue = inspection.producer_name
		? [inspection.producer_name, inspection.producer_version]
				.filter(Boolean)
				.join(" ")
		: t("common:notAvailable");

	const rows: [string, string][] = [
		[t("detail.metadata.irVersion"), String(inspection.ir_version)],
		[t("detail.metadata.opsetVersion"), String(inspection.opset_version)],
		[t("detail.metadata.producer"), producerValue],
		[t("detail.metadata.modelVersion"), String(inspection.model_version)],
		[t("detail.metadata.operations"), String(inspection.op_count)],
		[
			t("detail.metadata.parameters"),
			formatCompactNumber(
				inspection.param_count,
				i18n.resolvedLanguage ?? i18n.language,
			),
		],
	];

	if (inspection.domain) {
		rows.push([t("detail.metadata.domain"), inspection.domain]);
	}

	return (
		<aside className="w-[320px] shrink-0 overflow-y-auto border-l border-border bg-card p-[18px]">
			<RailSection title={t("detail.sections.file")}>
				<p className="break-all font-mono text-[11px] leading-[17px] text-muted-foreground">
					{model.filename}
				</p>
			</RailSection>

			<RailSection title={t("detail.sections.metadata")}>
				<dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-2 text-xs">
					{rows.map(([label, value]) => (
						<div key={label} className="contents">
							<dt className="text-muted-foreground">{label}</dt>
							<dd className="truncate text-right font-mono">{value}</dd>
						</div>
					))}
				</dl>
				{inspection.param_count === 0 && (
					<p className="mt-2 text-[11px] leading-4 text-muted-foreground/85">
						{t("detail.metadata.parametersNote")}
					</p>
				)}
			</RailSection>

			<RailSection title={t("detail.schema.inputs")}>
				<TensorList tensors={inspection.inputs} />
			</RailSection>

			<RailSection title={t("detail.schema.outputs")}>
				<TensorList tensors={inspection.outputs} />
			</RailSection>

			{inspection.doc_string && (
				<p className="text-[11px] leading-4 text-muted-foreground">
					{inspection.doc_string}
				</p>
			)}
		</aside>
	);
}

// ─── Dialog ───────────────────────────────────────────────────────────────────

type DetailView = "architecture" | "operators";

interface ModelDetailProps {
	model: ModelEntry | null;
	open: boolean;
	onOpenChange: (open: boolean) => void;
}

export function ModelDetail({ model, open, onOpenChange }: ModelDetailProps) {
	const { t } = useTranslation("models");
	const [inspection, setInspection] = useState<ModelInspection | null>(null);
	const [errorDetail, setErrorDetail] = useState<string | null>(null);
	const [loading, setLoading] = useState(false);
	const [view, setView] = useState<DetailView>("architecture");

	const filename = open && model ? model.filename : null;

	useEffect(() => {
		if (!filename) return;

		let cancelled = false;
		// eslint-disable-next-line react-hooks/set-state-in-effect -- synchronous loading flag before async fetch
		setLoading(true);

		inspectModel(filename)
			.then((data) => {
				if (!cancelled) {
					setInspection(data);
					setErrorDetail(null);
					setLoading(false);
				}
			})
			.catch((err: unknown) => {
				if (!cancelled) {
					setErrorDetail(getErrorMessage(err));
					setLoading(false);
				}
			});

		return () => {
			cancelled = true;
		};
	}, [filename]);

	const handleOpenChange = useCallback(
		(next: boolean) => {
			if (!next) {
				setInspection(null);
				setErrorDetail(null);
				setLoading(false);
				setView("architecture");
			}
			onOpenChange(next);
		},
		[onOpenChange],
	);

	const inspectErrorMessage = errorDetail
		? formatErrorWithPrefix(t("detail.error.inspectFailed"), errorDetail)
		: null;

	const views: { value: DetailView; labelKey: string }[] = [
		{ value: "architecture", labelKey: "detail.view.architecture" },
		{ value: "operators", labelKey: "detail.view.operators" },
	];

	return (
		<Dialog open={open} onOpenChange={handleOpenChange}>
			{/* The graph is the reason this opens, so it gets the room. */}
			<DialogContent className="grid h-[calc(100vh-2rem)] w-[calc(100vw-2rem)] max-w-[1600px] grid-rows-[auto_minmax(0,1fr)] gap-0 overflow-hidden p-0">
				<DialogHeader className="flex h-12 flex-row items-center gap-3 space-y-0 border-b border-border px-[18px] pr-12 text-left">
					<DialogTitle className="shrink-0 truncate font-mono text-[15px] font-semibold tracking-[-0.01em]">
						{model?.name ?? t("detail.titleFallback")}
					</DialogTitle>
					<DialogDescription className="sr-only">
						{model?.filename ?? ""}
					</DialogDescription>
					<span className="flex-1" />
					{inspection && (
						<div
							role="group"
							aria-label={t("detail.view.groupLabel")}
							className="flex h-8 shrink-0 items-center gap-0.5 rounded-lg border border-border bg-secondary/45 p-[3px]"
						>
							{views.map((option) => (
								<button
									key={option.value}
									type="button"
									aria-pressed={view === option.value}
									onClick={() => {
										setView(option.value);
									}}
									className={cn(
										"inline-flex h-6 items-center rounded-md px-2.5 text-xs transition-colors",
										view === option.value
											? "bg-primary/15 font-semibold text-primary"
											: "font-medium text-muted-foreground hover:text-foreground",
									)}
								>
									{t(option.labelKey)}
								</button>
							))}
						</div>
					)}
				</DialogHeader>

				{loading && (
					<div className="flex items-center justify-center">
						<Loader2 className="size-6 animate-spin text-muted-foreground" />
					</div>
				)}

				{!loading && inspectErrorMessage && (
					<div className="flex items-center justify-center">
						<p className="text-sm text-destructive">{inspectErrorMessage}</p>
					</div>
				)}

				{!loading && !inspectErrorMessage && inspection && model && (
					<div className="grid min-h-0 grid-cols-[minmax(0,1fr)_auto]">
						<div className="min-w-0">
							{view === "architecture" ? (
								<ReactFlowProvider>
									<GraphSection inspection={inspection} />
								</ReactFlowProvider>
							) : (
								<OperatorsSection inspection={inspection} />
							)}
						</div>
						<DetailRail model={model} inspection={inspection} />
					</div>
				)}
			</DialogContent>
		</Dialog>
	);
}
