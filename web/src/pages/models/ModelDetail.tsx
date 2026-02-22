import dagre from "@dagrejs/dagre";
import type { Edge, Node } from "@xyflow/react";
import {
	Background,
	BackgroundVariant,
	Controls,
	MiniMap,
	ReactFlow,
	ReactFlowProvider,
} from "@xyflow/react";
import { Loader2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { ModelEntry, ModelInspection } from "@/api/client";
import { inspectModel } from "@/api/client";
import { Badge } from "@/components/ui/badge";
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogHeader,
	DialogTitle,
} from "@/components/ui/dialog";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
	formatErrorWithPrefix,
	getErrorMessage,
} from "@/lib/presentation-error";
import { formatCompactNumber } from "@/lib/presentation-format";

const GRAPH_NODE_W = 180;
const GRAPH_NODE_H = 56;

function formatShape(shape: number[]): string {
	const dims = shape.map((d) => (d < 0 ? "?" : String(d)));
	return `[${dims.join(", ")}]`;
}

// ─── Graph layout ─────────────────────────────────────────────────────────────

interface OnnxGraphData {
	nodes: Node[];
	edges: Edge[];
}

function buildOnnxGraph(inspection: ModelInspection): OnnxGraphData {
	const rfNodes: Node[] = [];
	const rfEdges: Edge[] = [];

	const outputToNodeId = new Map<string, string>();
	const graphOutputNames = new Set(inspection.outputs.map((t) => t.name));

	for (let i = 0; i < inspection.nodes.length; i++) {
		const node = inspection.nodes[i];
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

	// Filter out initializer-like inputs for large models.
	// Initializers (weights/biases) have fully static shapes (all dims > 0).
	// Real model inputs have dynamic dimensions (dim <= 0 means unknown/batch).
	const isLargeModel = inspection.nodes.length > 50;
	for (const inp of inspection.inputs) {
		const hasDynamicDim = inp.shape.some((d) => d <= 0);
		if (isLargeModel && !hasDynamicDim && inp.shape.length > 0) {
			// Skip initializer-like input (fully static shape in large model)
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
		const outputNodeId = `output-${out.name}`;
		rfNodes.push({
			id: outputNodeId,
			position: { x: 0, y: 0 },
			data: {
				label: out.name,
				subtitle: `${out.data_type} ${formatShape(out.shape)}`,
			},
			type: "onnxIO",
		});
	}

	let edgeIdx = 0;
	for (let i = 0; i < inspection.nodes.length; i++) {
		const node = inspection.nodes[i];
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
				const outNodeId = `output-${outputTensor}`;
				rfEdges.push({
					id: `e-${String(edgeIdx++)}`,
					source: targetId,
					target: outNodeId,
				});
			}
		}
	}

	const g = new dagre.graphlib.Graph();
	g.setGraph({ rankdir: "TB", nodesep: 40, ranksep: 64 });
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

// ─── Custom ReactFlow node components ─────────────────────────────────────────

function OnnxOpNode({ data }: { data: { label: string; subtitle: string } }) {
	return (
		<div className="rounded-md border border-border bg-card px-3 py-2 text-center shadow-sm min-w-[140px]">
			<div className="text-xs font-semibold text-foreground leading-tight">
				{data.label}
			</div>
			{data.subtitle && (
				<div className="text-[10px] text-muted-foreground truncate max-w-[160px] mt-0.5">
					{data.subtitle}
				</div>
			)}
		</div>
	);
}

function OnnxIONode({ data }: { data: { label: string; subtitle: string } }) {
	return (
		<div className="rounded-md border-2 border-emerald-500/50 bg-emerald-500/10 px-3 py-2 text-center min-w-[140px]">
			<div className="text-xs font-semibold text-emerald-400 leading-tight">
				{data.label}
			</div>
			{data.subtitle && (
				<div className="text-[10px] text-muted-foreground truncate max-w-[160px] mt-0.5">
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

// ─── Metadata grid ────────────────────────────────────────────────────────────

function MetadataSection({
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

	const fields: [string, string][] = [
		[t("detail.metadata.name"), model.name],
		[t("detail.metadata.filename"), model.filename],
		[t("detail.metadata.irVersion"), String(inspection.ir_version)],
		[t("detail.metadata.opsetVersion"), String(inspection.opset_version)],
		[t("detail.metadata.producer"), producerValue],
		[t("detail.metadata.operations"), String(inspection.op_count)],
		[
			t("detail.metadata.parameters"),
			formatCompactNumber(
				inspection.param_count,
				i18n.resolvedLanguage ?? i18n.language,
			),
		],
		[t("detail.metadata.modelVersion"), String(inspection.model_version)],
	];

	if (inspection.domain) {
		fields.push([t("detail.metadata.domain"), inspection.domain]);
	}

	return (
		<div className="space-y-3">
			<div className="grid grid-cols-2 gap-x-6 gap-y-2 text-sm">
				{fields.map(([label, value]) => (
					<div key={label} className="flex justify-between gap-2">
						<span className="text-muted-foreground shrink-0">{label}</span>
						<span className="text-foreground font-mono text-xs truncate text-right">
							{value}
						</span>
					</div>
				))}
			</div>
			{inspection.doc_string && (
				<p className="text-xs text-muted-foreground border-t border-border pt-2 mt-2">
					{inspection.doc_string}
				</p>
			)}
		</div>
	);
}

// ─── I/O schema table ─────────────────────────────────────────────────────────

function IOSchemaSection({ inspection }: { inspection: ModelInspection }) {
	const { t } = useTranslation("models");

	return (
		<div className="space-y-4">
			<div>
				<h4 className="text-xs font-semibold text-muted-foreground uppercase tracking-wider mb-2">
					{t("detail.schema.inputs")}
				</h4>
				<div className="rounded-md border border-border overflow-hidden">
					<table className="w-full text-xs">
						<thead>
							<tr className="border-b border-border bg-muted/50">
								<th className="px-3 py-1.5 text-left font-medium text-muted-foreground">
									{t("detail.schema.name")}
								</th>
								<th className="px-3 py-1.5 text-left font-medium text-muted-foreground">
									{t("detail.schema.dataType")}
								</th>
								<th className="px-3 py-1.5 text-left font-medium text-muted-foreground">
									{t("detail.schema.shape")}
								</th>
							</tr>
						</thead>
						<tbody>
							{inspection.inputs.map((t) => (
								<tr
									key={t.name}
									className="border-b border-border last:border-0"
								>
									<td className="px-3 py-1.5 font-mono">{t.name}</td>
									<td className="px-3 py-1.5">
										<Badge
											variant="outline"
											className="text-[10px] px-1.5 py-0"
										>
											{t.data_type}
										</Badge>
									</td>
									<td className="px-3 py-1.5 font-mono text-muted-foreground">
										{formatShape(t.shape)}
									</td>
								</tr>
							))}
						</tbody>
					</table>
				</div>
			</div>

			<div>
				<h4 className="text-xs font-semibold text-muted-foreground uppercase tracking-wider mb-2">
					{t("detail.schema.outputs")}
				</h4>
				<div className="rounded-md border border-border overflow-hidden">
					<table className="w-full text-xs">
						<thead>
							<tr className="border-b border-border bg-muted/50">
								<th className="px-3 py-1.5 text-left font-medium text-muted-foreground">
									{t("detail.schema.name")}
								</th>
								<th className="px-3 py-1.5 text-left font-medium text-muted-foreground">
									{t("detail.schema.dataType")}
								</th>
								<th className="px-3 py-1.5 text-left font-medium text-muted-foreground">
									{t("detail.schema.shape")}
								</th>
							</tr>
						</thead>
						<tbody>
							{inspection.outputs.map((t) => (
								<tr
									key={t.name}
									className="border-b border-border last:border-0"
								>
									<td className="px-3 py-1.5 font-mono">{t.name}</td>
									<td className="px-3 py-1.5">
										<Badge
											variant="outline"
											className="text-[10px] px-1.5 py-0"
										>
											{t.data_type}
										</Badge>
									</td>
									<td className="px-3 py-1.5 font-mono text-muted-foreground">
										{formatShape(t.shape)}
									</td>
								</tr>
							))}
						</tbody>
					</table>
				</div>
			</div>
		</div>
	);
}

// ─── Graph visualization ──────────────────────────────────────────────────────

function GraphSection({ inspection }: { inspection: ModelInspection }) {
	const { t } = useTranslation("models");
	const tooLarge = inspection.nodes.length > 500;
	const { nodes, edges } = useMemo(
		() => (tooLarge ? { nodes: [], edges: [] } : buildOnnxGraph(inspection)),
		[inspection, tooLarge],
	);

	const miniMapNodeColor = useCallback(
		(n: { data: Record<string, unknown> }) => {
			const d = n.data as { label: string };
			if (d.label) return "#10b981";
			return "#888";
		},
		[],
	);

	if (tooLarge) {
		return (
			<div className="h-[420px] rounded-md border border-border flex items-center justify-center bg-background">
				<div className="text-center space-y-2">
					<p className="text-sm text-muted-foreground">
						{t("detail.graph.tooLarge", { count: inspection.nodes.length })}
					</p>
					<p className="text-xs text-muted-foreground">
						{t("detail.graph.tooLargeHint")}
					</p>
				</div>
			</div>
		);
	}

	return (
		<div className="h-[420px] rounded-md border border-border overflow-hidden bg-background">
			<ReactFlow
				nodes={nodes}
				edges={edges}
				nodeTypes={onnxNodeTypes}
				fitView
				nodesDraggable={false}
				nodesConnectable={false}
				elementsSelectable={false}
				panOnScroll
				zoomOnScroll
				proOptions={{ hideAttribution: true }}
				defaultEdgeOptions={{
					type: "smoothstep",
					style: { stroke: "var(--border)", strokeWidth: 1 },
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
					maskColor="rgba(0,0,0,0.5)"
					pannable
					zoomable
				/>
				<Controls className="!bg-card !border-border/50 !shadow-sm" />
			</ReactFlow>
		</div>
	);
}

// ─── Main dialog ──────────────────────────────────────────────────────────────

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
			}
			onOpenChange(next);
		},
		[onOpenChange],
	);

	const inspectErrorMessage = errorDetail
		? formatErrorWithPrefix(t("detail.error.inspectFailed"), errorDetail)
		: null;

	return (
		<Dialog open={open} onOpenChange={handleOpenChange}>
			<DialogContent className="max-w-3xl max-h-[90vh] overflow-y-auto">
				<DialogHeader>
					<DialogTitle className="text-base">
						{model?.name ?? t("detail.titleFallback")}
					</DialogTitle>
					<DialogDescription className="font-mono text-xs">
						{model?.filename ?? ""}
					</DialogDescription>
				</DialogHeader>

				{loading && (
					<div className="flex items-center justify-center py-12">
						<Loader2 className="size-6 animate-spin text-muted-foreground" />
					</div>
				)}

				{inspectErrorMessage && (
					<div className="flex items-center justify-center py-12">
						<p className="text-sm text-destructive">{inspectErrorMessage}</p>
					</div>
				)}

				{inspection && model && (
					<Tabs defaultValue="metadata" className="mt-2">
						<TabsList className="w-full">
							<TabsTrigger value="metadata" className="flex-1">
								{t("detail.tabs.metadata")}
							</TabsTrigger>
							<TabsTrigger value="io" className="flex-1">
								{t("detail.tabs.io")}
							</TabsTrigger>
							<TabsTrigger value="graph" className="flex-1">
								{t("detail.tabs.graph")}
							</TabsTrigger>
						</TabsList>

						<TabsContent value="metadata">
							<MetadataSection model={model} inspection={inspection} />
						</TabsContent>

						<TabsContent value="io">
							<IOSchemaSection inspection={inspection} />
						</TabsContent>

						<TabsContent value="graph">
							<ReactFlowProvider>
								<GraphSection inspection={inspection} />
							</ReactFlowProvider>
						</TabsContent>
					</Tabs>
				)}
			</DialogContent>
		</Dialog>
	);
}
