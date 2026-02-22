import dagre from '@dagrejs/dagre';
import type { Node, Edge } from '@xyflow/react';

import type { PipelineNodeData, NodeTypeName } from '@/types';
import { useNodeDefinitions } from '@/stores/node-definitions-store';

const NODE_WIDTH = 280;
const PORT_ROW_HEIGHT = 36;
const TITLE_HEIGHT = 60;

function estimateNodeHeight(nodeType: NodeTypeName): number {
  const descriptors = useNodeDefinitions.getState().descriptors;
  const desc = descriptors.find((d) => d.node_type === nodeType);
  if (!desc) return 120;
  const maxPorts = Math.max(desc.inputs.length, desc.outputs.length);
  return TITLE_HEIGHT + maxPorts * PORT_ROW_HEIGHT;
}

export function computeLayout(
  nodes: Node<PipelineNodeData>[],
  edges: Edge[],
): Record<string, { x: number; y: number }> {
  const g = new dagre.graphlib.Graph();
  g.setGraph({ rankdir: 'LR', nodesep: 50, ranksep: 80, edgesep: 50 });
  g.setDefaultEdgeLabel(() => ({}));

  for (const node of nodes) {
    const height = estimateNodeHeight(node.data.nodeType);
    g.setNode(node.id, { width: NODE_WIDTH, height });
  }

  for (const edge of edges) {
    g.setEdge(edge.source, edge.target);
  }

  dagre.layout(g);

  const positions: Record<string, { x: number; y: number }> = {};
  for (const node of nodes) {
    const dagreNode = g.node(node.id);
    if (dagreNode) {
      // dagre gives center coordinates; React Flow uses top-left
      positions[node.id] = {
        x: dagreNode.x - NODE_WIDTH / 2,
        y: dagreNode.y - (dagreNode.height as number) / 2,
      };
    }
  }

  return positions;
}
