import type { Edge, Node } from '@xyflow/react';
import { create } from 'zustand';
import type {
  NodeTypeName,
  PipelineNodeData,
  PortType,
  Workflow,
  WorkflowConnection,
  WorkflowInterface,
  WorkflowPort,
} from '../types';
import { useNodeDefinitions } from './node-definitions-store';

const MAX_HISTORY = 50;

export function normalizeParams(
  params: Record<string, unknown>,
): Record<string, string | number | boolean> {
  const out: Record<string, string | number | boolean> = {};
  for (const [k, v] of Object.entries(params)) {
    if (typeof v === 'string' || typeof v === 'number' || typeof v === 'boolean') {
      out[k] = v;
    } else {
      out[k] = JSON.stringify(v);
    }
  }
  return out;
}

interface HistoryEntry {
  nodes: Node<PipelineNodeData>[];
  edges: Edge[];
}

interface WorkflowState {
  nodes: Node<PipelineNodeData>[];
  edges: Edge[];

  past: HistoryEntry[];
  future: HistoryEntry[];

  setNodes: (nodes: Node<PipelineNodeData>[]) => void;
  setEdges: (edges: Edge[]) => void;
  addNode: (node: Node<PipelineNodeData>) => void;
  removeNode: (nodeId: string) => void;
  updateNodeParams: (
    nodeId: string,
    params: Record<string, string | number | boolean>,
  ) => void;
  addEdge: (edge: Edge) => void;
  removeEdge: (edgeId: string) => void;
  loadWorkflow: (
    workflow: Workflow,
    nodePositions?: Record<string, { x: number; y: number }>,
  ) => void;
  clear: () => void;
  undo: () => void;
  redo: () => void;

  currentFile: { filename: string; name: string; description: string } | null;
  setCurrentFile: (file: { filename: string; name: string; description: string } | null) => void;

  exportWorkflow: () => Workflow;
}

interface DynamicPortConfig {
  name: string;
  port_type: string;
  default_value?: unknown;
  [key: string]: unknown;
}

const PORT_TYPE_VALUES = new Set<PortType>([
  'VideoFrames',
  'Metadata',
  'Model',
  'Int',
  'Float',
  'Str',
  'Bool',
  'Path',
  'WorkflowPath',
]);

const STRUCTURED_EXPORT_PARAM_KEYS = new Set<string>([
  'ports',
  'interface_inputs',
  'interface_outputs',
]);

function parseStructuredExportParam(key: string, value: string): unknown {
  if (!STRUCTURED_EXPORT_PARAM_KEYS.has(key)) return value;
  try {
    return JSON.parse(value);
  } catch {
    return value;
  }
}

function snapshot(state: { nodes: Node<PipelineNodeData>[]; edges: Edge[] }): HistoryEntry {
  return {
    nodes: structuredClone(state.nodes),
    edges: structuredClone(state.edges),
  };
}

function parseIfacePorts(params: Record<string, string | number | boolean>, key: string): WorkflowPort[] {
  const raw = params[key];
  if (!raw || typeof raw !== 'string') return [];
  try {
    return JSON.parse(raw) as WorkflowPort[];
  } catch {
    return [];
  }
}

function toPortType(value: unknown): PortType | undefined {
  if (typeof value !== 'string') return undefined;
  if (!PORT_TYPE_VALUES.has(value as PortType)) return undefined;
  return value as PortType;
}

function resolveDescriptorPortType(
  port: { port_type: string; dynamic_type_param: string | null },
  params: Record<string, string | number | boolean>,
): PortType | undefined {
  if (port.dynamic_type_param) {
    const dynamicType = toPortType(params[port.dynamic_type_param]);
    if (dynamicType) return dynamicType;
  }
  return toPortType(port.port_type);
}

function parseStringTemplateDynamicInputs(
  params: Record<string, string | number | boolean>,
): WorkflowPort[] {
  const raw = params.num_input;
  const parsed = typeof raw === 'number' ? raw : Number(raw ?? 0);
  if (!Number.isFinite(parsed)) return [];
  const count = Math.max(0, Math.floor(parsed));
  return Array.from({ length: count }, (_, index) => ({
    name: `str${String(index)}`,
    port_type: 'Str',
  }));
}

function resolvePortType(
  nodes: Node<PipelineNodeData>[],
  edge: Edge,
): PortType {
  const descriptors = useNodeDefinitions.getState().descriptors;

  const sourceNode = nodes.find((n) => n.id === edge.source);
  if (sourceNode) {
    const sourceType = sourceNode.data.nodeType as NodeTypeName;
    const desc = descriptors.find((d) => d.node_type === sourceType);
    if (desc) {
      const outputPort = desc.outputs.find((p) => p.name === edge.sourceHandle);
      if (outputPort) {
        const resolved = resolveDescriptorPortType(outputPort, sourceNode.data.params);
        if (resolved) return resolved;
      }
    }
    if (sourceType === 'Workflow') {
      const match = parseIfacePorts(sourceNode.data.params, 'interface_outputs')
        .find((p) => p.name === edge.sourceHandle);
      if (match) return match.port_type as PortType;
    }
    if (sourceType === 'WorkflowInput') {
      const dynPorts = parseDynPorts(sourceNode.data.params);
      const match = dynPorts.find((p) => p.name === edge.sourceHandle);
      if (match) return match.port_type as PortType;
    }
  }

  const targetNode = nodes.find((n) => n.id === edge.target);
  if (targetNode) {
    const targetType = targetNode.data.nodeType as NodeTypeName;
    const desc = descriptors.find((d) => d.node_type === targetType);
    if (desc) {
      const inputPort = desc.inputs.find((p) => p.name === edge.targetHandle);
      if (inputPort) {
        const resolved = resolveDescriptorPortType(inputPort, targetNode.data.params);
        if (resolved) return resolved;
      }
    }
    if (targetType === 'Workflow') {
      const match = parseIfacePorts(targetNode.data.params, 'interface_inputs')
        .find((p) => p.name === edge.targetHandle);
      if (match) return match.port_type as PortType;
    }
    if (targetType === 'StringTemplate') {
      const match = parseStringTemplateDynamicInputs(targetNode.data.params)
        .find((p) => p.name === edge.targetHandle);
      if (match) return match.port_type as PortType;
    }
    if (targetType === 'WorkflowOutput') {
      const dynPorts = parseDynPorts(targetNode.data.params);
      const match = dynPorts.find((p) => p.name === edge.targetHandle);
      if (match) return match.port_type as PortType;
    }
  }

  if (edge.data && typeof edge.data === 'object' && 'port_type' in edge.data) {
    return edge.data.port_type as PortType;
  }

  return 'Str';
}

function parseDynPorts(params: Record<string, string | number | boolean>): WorkflowPort[] {
  const raw = params.ports;
  if (!raw || typeof raw !== 'string') return [];
  try {
    return JSON.parse(raw) as WorkflowPort[];
  } catch {
    return [];
  }
}

function parseDynamicPortConfig(raw: string | number | boolean | undefined): DynamicPortConfig[] {
  if (typeof raw !== 'string') return [];
  try {
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];

    return parsed.flatMap((entry, index) => {
      if (!entry || typeof entry !== 'object') return [];

      const rawPort = entry as Record<string, unknown>;
      const name = typeof rawPort.name === 'string'
        ? rawPort.name
        : `port_${String(index + 1)}`;
      const port_type = typeof rawPort.port_type === 'string'
        ? rawPort.port_type
        : 'Str';

      return [{ ...rawPort, name, port_type }];
    });
  } catch {
    return [];
  }
}

function normalizeDynamicPortName(rawName: string, index: number): string {
  const trimmed = rawName.trim();
  return trimmed.length > 0 ? trimmed : `port_${String(index + 1)}`;
}

function normalizeDynamicPorts(
  nextPorts: DynamicPortConfig[],
  previousPorts: DynamicPortConfig[],
): DynamicPortConfig[] {
  const normalized = nextPorts.map((port, index) => ({
    ...port,
    name: normalizeDynamicPortName(port.name, index),
  }));

  const reservedNames = new Set<string>();
  const lockedIndexes = new Set<number>();

  normalized.forEach((port, index) => {
    const previousName = previousPorts[index]?.name;
    if (!previousName) return;
    if (port.name !== previousName) return;
    if (reservedNames.has(previousName)) return;
    reservedNames.add(previousName);
    lockedIndexes.add(index);
  });

  const nextSuffix = new Map<string, number>();

  return normalized.map((port, index) => {
    if (lockedIndexes.has(index)) return port;

    const base = port.name;
    let candidate = base;
    let suffix = nextSuffix.get(base) ?? 2;

    while (reservedNames.has(candidate)) {
      candidate = `${base}_${String(suffix)}`;
      suffix += 1;
    }

    nextSuffix.set(base, suffix);
    reservedNames.add(candidate);

    if (candidate === port.name) return port;
    return { ...port, name: candidate };
  });
}

function buildDynamicPortRenameMap(
  previousPorts: DynamicPortConfig[],
  nextPorts: DynamicPortConfig[],
): Map<string, string> {
  const remap = new Map<string, string>();
  const overlap = Math.min(previousPorts.length, nextPorts.length);

  for (let index = 0; index < overlap; index += 1) {
    const previousName = previousPorts[index]?.name;
    const nextName = nextPorts[index]?.name;
    if (!previousName || !nextName || previousName === nextName) continue;
    if (!remap.has(previousName)) {
      remap.set(previousName, nextName);
    }
  }

  return remap;
}

function remapDynamicPortEdgeHandles(
  edges: Edge[],
  nodeId: string,
  side: 'source' | 'target',
  remap: Map<string, string>,
): Edge[] {
  if (remap.size === 0) return edges;

  return edges.map((edge) => {
    if (side === 'source') {
      if (edge.source !== nodeId || !edge.sourceHandle) return edge;
      const nextHandle = remap.get(edge.sourceHandle);
      if (!nextHandle) return edge;
      return { ...edge, sourceHandle: nextHandle };
    }

    if (edge.target !== nodeId || !edge.targetHandle) return edge;
    const nextHandle = remap.get(edge.targetHandle);
    if (!nextHandle) return edge;
    return { ...edge, targetHandle: nextHandle };
  });
}

function extractWorkflowInterface(
  nodes: Node<PipelineNodeData>[],
): WorkflowInterface | null {
  const inputNode = nodes.find((n) => n.data.nodeType === 'WorkflowInput');
  const outputNode = nodes.find((n) => n.data.nodeType === 'WorkflowOutput');

  if (!inputNode && !outputNode) return null;

  const parsePortConfig = (params: Record<string, string | number | boolean>): WorkflowPort[] => {
    const portsRaw = params.ports;
    if (!portsRaw || typeof portsRaw !== 'string') return [];
    try {
      const parsed = JSON.parse(portsRaw) as Array<{ name: string; port_type: string; default_value?: unknown }>;
      return parsed.map((p) => ({
        name: p.name,
        port_type: p.port_type,
        ...(p.default_value !== undefined ? { default_value: p.default_value } : {}),
      }));
    } catch {
      return [];
    }
  };

  return {
    inputs: inputNode ? parsePortConfig(inputNode.data.params) : [],
    outputs: outputNode ? parsePortConfig(outputNode.data.params) : [],
  };
}

export const useWorkflowStore = create<WorkflowState>((set, get) => ({
  nodes: [],
  edges: [],
  past: [],
  future: [],
  currentFile: null,

  setNodes: (nodes) => {
    const state = get();
    set({
      past: [...state.past, snapshot(state)].slice(-MAX_HISTORY),
      future: [],
      nodes,
    });
  },

  setEdges: (edges) => {
    const state = get();
    set({
      past: [...state.past, snapshot(state)].slice(-MAX_HISTORY),
      future: [],
      edges,
    });
  },

  addNode: (node) => {
    const state = get();
    set({
      past: [...state.past, snapshot(state)].slice(-MAX_HISTORY),
      future: [],
      nodes: [...state.nodes, node],
    });
  },

  removeNode: (nodeId) => {
    const state = get();
    set({
      past: [...state.past, snapshot(state)].slice(-MAX_HISTORY),
      future: [],
      nodes: state.nodes.filter((n) => n.id !== nodeId),
      edges: state.edges.filter(
        (e) => e.source !== nodeId && e.target !== nodeId,
      ),
    });
  },

  updateNodeParams: (nodeId, params) => {
    const state = get();

    let nextParams = params;
    let nextEdges = state.edges;

    const targetNode = state.nodes.find((n) => n.id === nodeId);
    const nodeType = targetNode?.data.nodeType;

    if (
      targetNode
      && (nodeType === 'WorkflowInput' || nodeType === 'WorkflowOutput')
      && params.ports !== undefined
    ) {
      const previousPorts = parseDynamicPortConfig(targetNode.data.params.ports);
      const requestedPorts = parseDynamicPortConfig(params.ports);
      const normalizedPorts = normalizeDynamicPorts(requestedPorts, previousPorts);
      const remap = buildDynamicPortRenameMap(previousPorts, normalizedPorts);

      nextParams = {
        ...params,
        ports: JSON.stringify(normalizedPorts),
      };

      nextEdges = remapDynamicPortEdgeHandles(
        state.edges,
        nodeId,
        nodeType === 'WorkflowInput' ? 'source' : 'target',
        remap,
      );
    }

    set({
      past: [...state.past, snapshot(state)].slice(-MAX_HISTORY),
      future: [],
      edges: nextEdges,
      nodes: state.nodes.map((n) =>
        n.id === nodeId
          ? { ...n, data: { ...n.data, params: { ...n.data.params, ...nextParams } } }
          : n,
      ),
    });
  },

  addEdge: (edge) => {
    const state = get();
    set({
      past: [...state.past, snapshot(state)].slice(-MAX_HISTORY),
      future: [],
      edges: [...state.edges, edge],
    });
  },

  removeEdge: (edgeId) => {
    const state = get();
    set({
      past: [...state.past, snapshot(state)].slice(-MAX_HISTORY),
      future: [],
      edges: state.edges.filter((e) => e.id !== edgeId),
    });
  },

  loadWorkflow: (workflow, nodePositions) => {
    const state = get();
    const nodes: Node<PipelineNodeData>[] = workflow.nodes.map((wn, i) => ({
      id: wn.id,
      type: 'pipeline',
      position: nodePositions?.[wn.id] ?? { x: 250 * i, y: 100 },
      data: {
        nodeType: wn.node_type as NodeTypeName,
        params: normalizeParams(wn.params),
      },
    }));

    const edges: Edge[] = workflow.connections.map((conn, i) => ({
      id: `e-${conn.from_node}-${conn.from_port}-${conn.to_node}-${conn.to_port}-${String(i)}`,
      source: conn.from_node,
      sourceHandle: conn.from_port,
      target: conn.to_node,
      targetHandle: conn.to_port,
      data: { port_type: conn.port_type },
    }));

    set({
      past: [...state.past, snapshot(state)].slice(-MAX_HISTORY),
      future: [],
      nodes,
      edges,
      currentFile: null,
    });
  },

  clear: () => {
    const state = get();
    set({
      past: [...state.past, snapshot(state)].slice(-MAX_HISTORY),
      future: [],
      nodes: [],
      edges: [],
      currentFile: null,
    });
  },

  setCurrentFile: (file) => set({ currentFile: file }),

  undo: () => {
    const state = get();
    const previous = state.past.at(-1);
    if (!previous) return;
    set({
      past: state.past.slice(0, -1),
      future: [snapshot(state), ...state.future],
      nodes: previous.nodes,
      edges: previous.edges,
    });
  },

  redo: () => {
    const state = get();
    const next = state.future.at(0);
    if (!next) return;
    set({
      past: [...state.past, snapshot(state)],
      future: state.future.slice(1),
      nodes: next.nodes,
      edges: next.edges,
    });
  },

  exportWorkflow: (): Workflow => {
    const { nodes, edges } = get();

    const workflowNodes = nodes.map((n) => {
      const params: Record<string, unknown> = {};
      for (const [k, v] of Object.entries(n.data.params)) {
        if (typeof v === 'string') {
          params[k] = parseStructuredExportParam(k, v);
        } else {
          params[k] = v;
        }
      }
      return { id: n.id, node_type: n.data.nodeType, params };
    });

    const connections: WorkflowConnection[] = edges.map((edge) => ({
      from_node: edge.source,
      from_port: edge.sourceHandle ?? '',
      to_node: edge.target,
      to_port: edge.targetHandle ?? '',
      port_type: resolvePortType(nodes, edge),
    }));

    const wfInterface = extractWorkflowInterface(nodes);
    if (wfInterface) {
      return { nodes: workflowNodes, connections, interface: wfInterface };
    }

    return { nodes: workflowNodes, connections };
  },
}));
