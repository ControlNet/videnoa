import type { Edge, Node } from '@xyflow/react';
import { beforeEach, describe, expect, it } from 'vitest';

import presetJson from '../../../../presets/anime-2x-interpolation-2x.json';
import type { PipelineNodeData, Workflow, WorkflowPort } from '../../types';
import type { NodeDescriptor } from '../node-definitions-store';
import { useNodeDefinitions } from '../node-definitions-store';
import { useWorkflowStore } from '../workflow-store';

const preset: Workflow = presetJson.workflow as Workflow;

const MOCK_DESCRIPTORS: NodeDescriptor[] = [
  {
    node_type: 'VideoInput',
    display_name: 'Video Input',
    category: 'input',
    accent_color: '#8B5CF6',
    icon: 'file-video',
    inputs: [
      { name: 'path', port_type: 'Path', direction: 'param', required: true, default_value: null, ui_hint: null, enum_options: null, dynamic_type_param: null },
    ],
    outputs: [
      { name: 'frames', port_type: 'VideoFrames', direction: 'stream', required: true, default_value: null, ui_hint: null, enum_options: null, dynamic_type_param: null },
      { name: 'metadata', port_type: 'Metadata', direction: 'stream', required: true, default_value: null, ui_hint: null, enum_options: null, dynamic_type_param: null },
      { name: 'source_path', port_type: 'Path', direction: 'param', required: true, default_value: null, ui_hint: null, enum_options: null, dynamic_type_param: null },
    ],
  },
  {
    node_type: 'SuperResolution',
    display_name: 'Super Resolution',
    category: 'processing',
    accent_color: '#F97316',
    icon: 'microscope',
    inputs: [
      { name: 'frames', port_type: 'VideoFrames', direction: 'stream', required: true, default_value: null, ui_hint: null, enum_options: null, dynamic_type_param: null },
      { name: 'model_path', port_type: 'Path', direction: 'param', required: true, default_value: null, ui_hint: null, enum_options: null, dynamic_type_param: null },
      { name: 'scale', port_type: 'Int', direction: 'param', required: false, default_value: 4, ui_hint: null, enum_options: null, dynamic_type_param: null },
      { name: 'tile_size', port_type: 'Int', direction: 'param', required: false, default_value: 0, ui_hint: null, enum_options: null, dynamic_type_param: null },
      { name: 'backend', port_type: 'Str', direction: 'param', required: false, default_value: 'cuda', ui_hint: null, enum_options: ['cuda', 'tensorrt'], dynamic_type_param: null },
      { name: 'use_iobinding', port_type: 'Bool', direction: 'param', required: false, default_value: false, ui_hint: null, enum_options: null, dynamic_type_param: null },
    ],
    outputs: [
      { name: 'frames', port_type: 'VideoFrames', direction: 'stream', required: true, default_value: null, ui_hint: null, enum_options: null, dynamic_type_param: null },
    ],
  },
  {
    node_type: 'FrameInterpolation',
    display_name: 'Frame Interpolation',
    category: 'processing',
    accent_color: '#06B6D4',
    icon: 'film',
    inputs: [
      { name: 'frames', port_type: 'VideoFrames', direction: 'stream', required: true, default_value: null, ui_hint: null, enum_options: null, dynamic_type_param: null },
      { name: 'model_path', port_type: 'Path', direction: 'param', required: true, default_value: null, ui_hint: null, enum_options: null, dynamic_type_param: null },
      { name: 'multiplier', port_type: 'Int', direction: 'param', required: false, default_value: 2, ui_hint: null, enum_options: null, dynamic_type_param: null },
      { name: 'backend', port_type: 'Str', direction: 'param', required: false, default_value: 'cuda', ui_hint: null, enum_options: ['cuda', 'tensorrt'], dynamic_type_param: null },
      { name: 'use_iobinding', port_type: 'Bool', direction: 'param', required: false, default_value: false, ui_hint: null, enum_options: null, dynamic_type_param: null },
    ],
    outputs: [
      { name: 'frames', port_type: 'VideoFrames', direction: 'stream', required: true, default_value: null, ui_hint: null, enum_options: null, dynamic_type_param: null },
    ],
  },
  {
    node_type: 'VideoOutput',
    display_name: 'Video Output',
    category: 'output',
    accent_color: '#22C55E',
    icon: 'hard-drive',
    inputs: [
      { name: 'frames', port_type: 'VideoFrames', direction: 'stream', required: true, default_value: null, ui_hint: null, enum_options: null, dynamic_type_param: null },
      { name: 'source_path', port_type: 'Path', direction: 'param', required: true, default_value: null, ui_hint: null, enum_options: null, dynamic_type_param: null },
      { name: 'output_path', port_type: 'Path', direction: 'param', required: true, default_value: null, ui_hint: null, enum_options: null, dynamic_type_param: null },
      { name: 'codec', port_type: 'Str', direction: 'param', required: false, default_value: 'libx265', ui_hint: null, enum_options: ['libx265', 'libx264'], dynamic_type_param: null },
      { name: 'crf', port_type: 'Int', direction: 'param', required: false, default_value: 18, ui_hint: null, enum_options: null, dynamic_type_param: null },
      { name: 'pixel_format', port_type: 'Str', direction: 'param', required: false, default_value: 'yuv420p10le', ui_hint: null, enum_options: ['yuv420p10le', 'yuv420p'], dynamic_type_param: null },
      { name: 'width', port_type: 'Int', direction: 'param', required: true, default_value: null, ui_hint: null, enum_options: null, dynamic_type_param: null },
      { name: 'height', port_type: 'Int', direction: 'param', required: true, default_value: null, ui_hint: null, enum_options: null, dynamic_type_param: null },
      { name: 'fps', port_type: 'Str', direction: 'param', required: true, default_value: null, ui_hint: null, enum_options: null, dynamic_type_param: null },
    ],
    outputs: [
      { name: 'output_path', port_type: 'Path', direction: 'param', required: true, default_value: null, ui_hint: null, enum_options: null, dynamic_type_param: null },
    ],
  },
  {
    node_type: 'WorkflowInput',
    display_name: 'Workflow Input',
    category: 'workflow',
    accent_color: '#8B5CF6',
    icon: 'arrow-down-to-line',
    inputs: [],
    outputs: [],
  },
  {
    node_type: 'WorkflowOutput',
    display_name: 'Workflow Output',
    category: 'workflow',
    accent_color: '#8B5CF6',
    icon: 'arrow-up-from-line',
    inputs: [],
    outputs: [],
  },
  {
    node_type: 'StringTemplate',
    display_name: 'String Template',
    category: 'utility',
    accent_color: '#6366F1',
    icon: 'braces',
    inputs: [
      { name: 'num_input', port_type: 'Int', direction: 'param', required: false, default_value: 0, ui_hint: null, enum_options: null, dynamic_type_param: null },
      { name: 'template', port_type: 'Str', direction: 'param', required: false, default_value: '', ui_hint: null, enum_options: null, dynamic_type_param: null },
      { name: 'strict', port_type: 'Bool', direction: 'param', required: false, default_value: true, ui_hint: null, enum_options: null, dynamic_type_param: null },
    ],
    outputs: [
      { name: 'value', port_type: 'Str', direction: 'param', required: true, default_value: null, ui_hint: null, enum_options: null, dynamic_type_param: null },
    ],
  },
  {
    node_type: 'TypeConversion',
    display_name: 'Type Conversion',
    category: 'utility',
    accent_color: '#6366F1',
    icon: 'arrow-left-right',
    inputs: [
      { name: 'input_type', port_type: 'Str', direction: 'param', required: false, default_value: 'Int', ui_hint: null, enum_options: ['Int', 'Float', 'Str', 'Bool', 'Path'], dynamic_type_param: null },
      { name: 'output_type', port_type: 'Str', direction: 'param', required: false, default_value: 'Int', ui_hint: null, enum_options: ['Int', 'Float', 'Str', 'Bool', 'Path'], dynamic_type_param: null },
      { name: 'value', port_type: 'Int', direction: 'param', required: true, default_value: null, ui_hint: null, enum_options: null, dynamic_type_param: 'input_type' },
    ],
    outputs: [
      { name: 'value', port_type: 'Int', direction: 'param', required: true, default_value: null, ui_hint: null, enum_options: null, dynamic_type_param: 'output_type' },
    ],
  },
];

function makeNode(
  id: string,
  nodeType: PipelineNodeData['nodeType'],
  params: Record<string, string | number | boolean> = {},
): Node<PipelineNodeData> {
  return {
    id,
    type: 'pipeline',
    position: { x: 0, y: 0 },
    data: { nodeType, params },
  };
}

function makeEdge(
  id: string,
  source: string,
  target: string,
  sourceHandle: string,
  targetHandle: string,
): Edge {
  return { id, source, target, sourceHandle, targetHandle };
}

beforeEach(() => {
  useNodeDefinitions.setState({ descriptors: MOCK_DESCRIPTORS, loading: false, error: null });
  useWorkflowStore.setState({
    nodes: [],
    edges: [],
    past: [],
    future: [],
  });
});

describe('exportWorkflow', () => {
  it('produces correct JSON structure', () => {
    const nodes = [
      makeNode('input', 'VideoInput', {}),
      makeNode('sr', 'SuperResolution', { model_path: 'test.onnx', scale: 2, tile_size: 0 }),
      makeNode('output', 'VideoOutput', { codec: 'libx265', crf: 18 }),
    ];
    const edges = [
      makeEdge('e1', 'input', 'sr', 'frames', 'frames'),
      makeEdge('e2', 'sr', 'output', 'frames', 'frames'),
    ];
    useWorkflowStore.setState({ nodes, edges });

    const workflow = useWorkflowStore.getState().exportWorkflow();

    expect(workflow.nodes).toHaveLength(3);
    expect(workflow.connections).toHaveLength(2);
    expect(workflow.nodes[0]).toEqual({
      id: 'input',
      node_type: 'VideoInput',
      params: {},
    });
    expect(workflow.connections[0]).toMatchObject({
      from_node: 'input',
      from_port: 'frames',
      to_node: 'sr',
      to_port: 'frames',
    });
  });

  it('sets port_type VideoFrames for frames edges', () => {
    const nodes = [
      makeNode('input', 'VideoInput'),
      makeNode('sr', 'SuperResolution', { model_path: 'x.onnx', scale: 2, tile_size: 0 }),
    ];
    const edges = [makeEdge('e1', 'input', 'sr', 'frames', 'frames')];
    useWorkflowStore.setState({ nodes, edges });

    const workflow = useWorkflowStore.getState().exportWorkflow();

    expect(workflow.connections[0].port_type).toBe('VideoFrames');
  });

  it('resolves TypeConversion output value type from output_type param', () => {
    const nodes = [
      makeNode('conv', 'TypeConversion', { input_type: 'Str', output_type: 'Bool' }),
      makeNode('out', 'VideoOutput'),
    ];
    const edges = [makeEdge('e1', 'conv', 'out', 'value', 'codec')];
    useWorkflowStore.setState({ nodes, edges });

    const workflow = useWorkflowStore.getState().exportWorkflow();

    expect(workflow.connections[0].port_type).toBe('Bool');
  });

  it('uses dynamic output_type when it is a recognized PortType value', () => {
    const nodes = [
      makeNode('conv', 'TypeConversion', { input_type: 'Str', output_type: 'VideoFrames' }),
      makeNode('out', 'VideoOutput'),
    ];
    const edges = [makeEdge('e1', 'conv', 'out', 'value', 'codec')];
    useWorkflowStore.setState({ nodes, edges });

    const workflow = useWorkflowStore.getState().exportWorkflow();

    expect(workflow.connections[0].port_type).toBe('VideoFrames');
  });

  it('resolves StringTemplate dynamic input handles to Str', () => {
    const nodes = [
      makeNode('input', 'VideoInput'),
      makeNode('tpl', 'StringTemplate', { num_input: 2, template: '{str0}-{str1}', strict: true }),
    ];
    const edges = [makeEdge('e1', 'input', 'tpl', 'missing_source_port', 'str1')];
    useWorkflowStore.setState({ nodes, edges });

    const workflow = useWorkflowStore.getState().exportWorkflow();

    expect(workflow.connections[0].port_type).toBe('Str');
  });

  it('resolves TypeConversion input value type from input_type param', () => {
    const nodes = [
      makeNode('input', 'VideoInput'),
      makeNode('conv', 'TypeConversion', { input_type: 'Path', output_type: 'Str' }),
    ];
    const edges = [makeEdge('e1', 'input', 'conv', 'missing_source_port', 'value')];
    useWorkflowStore.setState({ nodes, edges });

    const workflow = useWorkflowStore.getState().exportWorkflow();

    expect(workflow.connections[0].port_type).toBe('Path');
  });

  it('keeps unknown handle fallback as Str', () => {
    const nodes = [
      makeNode('input', 'VideoInput'),
      makeNode('tpl', 'StringTemplate', { num_input: 1, template: '{str0}', strict: true }),
    ];
    const edges = [makeEdge('e1', 'input', 'tpl', 'missing_source_port', 'str3')];
    useWorkflowStore.setState({ nodes, edges });

    const workflow = useWorkflowStore.getState().exportWorkflow();

    expect(workflow.connections[0].port_type).toBe('Str');
  });

  it('preserves JSON-looking literal strings for non-allowlisted params', () => {
    const literal = '{"not":"structured-metadata"}';
    const nodes = [
      makeNode('tpl', 'StringTemplate', { num_input: 1, template: literal, strict: true }),
    ];
    useWorkflowStore.setState({ nodes, edges: [] });

    const workflow = useWorkflowStore.getState().exportWorkflow();

    expect(workflow.nodes[0].params.template).toBe(literal);
  });

  it('parses allowlisted structured params back to arrays/objects', () => {
    const portsJson = JSON.stringify([
      { name: 'input', port_type: 'Path', default_value: '/tmp/video.mkv' },
    ]);
    const ifaceOutputsJson = JSON.stringify([
      { name: 'result', port_type: 'Path' },
    ]);
    const nodes = [
      makeNode('wf-input', 'WorkflowInput', { ports: portsJson }),
      makeNode('wf', 'Workflow', { interface_outputs: ifaceOutputsJson }),
    ];
    useWorkflowStore.setState({ nodes, edges: [] });

    const workflow = useWorkflowStore.getState().exportWorkflow();

    expect(workflow.nodes[0].params.ports).toEqual([
      { name: 'input', port_type: 'Path', default_value: '/tmp/video.mkv' },
    ]);
    expect(workflow.nodes[1].params.interface_outputs).toEqual([
      { name: 'result', port_type: 'Path' },
    ]);
  });
});

describe('updateNodeParams dynamic WorkflowIO ports', () => {
  it('remaps WorkflowInput source handles when a dynamic port is renamed', () => {
    const nodes = [
      makeNode('wf-input', 'WorkflowInput', {
        ports: JSON.stringify([{ name: 'input_path', port_type: 'Path' }]),
      }),
      makeNode('video-output', 'VideoOutput'),
    ];
    const edges = [makeEdge('e1', 'wf-input', 'video-output', 'input_path', 'source_path')];
    useWorkflowStore.setState({ nodes, edges });

    useWorkflowStore.getState().updateNodeParams('wf-input', {
      ports: JSON.stringify([{ name: 'source_path', port_type: 'Path' }]),
    });

    const state = useWorkflowStore.getState();
    const updatedNode = state.nodes.find((node) => node.id === 'wf-input');
    const updatedPorts = JSON.parse(String(updatedNode?.data.params.ports)) as WorkflowPort[];

    expect(updatedPorts[0].name).toBe('source_path');
    expect(state.edges[0].sourceHandle).toBe('source_path');
  });

  it('keeps export/load valid after WorkflowOutput dynamic port rename', () => {
    const nodes = [
      makeNode('video-input', 'VideoInput'),
      makeNode('wf-output', 'WorkflowOutput', {
        ports: JSON.stringify([{ name: 'result', port_type: 'Path' }]),
      }),
    ];
    const edges = [makeEdge('e1', 'video-input', 'wf-output', 'source_path', 'result')];
    useWorkflowStore.setState({ nodes, edges });

    useWorkflowStore.getState().updateNodeParams('wf-output', {
      ports: JSON.stringify([{ name: 'output_path', port_type: 'Path' }]),
    });

    const exported = useWorkflowStore.getState().exportWorkflow();
    expect(exported.connections[0].to_port).toBe('output_path');

    useWorkflowStore.getState().loadWorkflow(exported);

    const reloadedState = useWorkflowStore.getState();
    expect(reloadedState.edges[0].targetHandle).toBe('output_path');

    const reloadedNode = reloadedState.nodes.find((node) => node.id === 'wf-output');
    const reloadedPorts = JSON.parse(String(reloadedNode?.data.params.ports)) as WorkflowPort[];
    expect(reloadedPorts[0].name).toBe('output_path');
  });

  it('resolves duplicate WorkflowInput rename deterministically without duplicate handles', () => {
    const nodes = [
      makeNode('wf-input', 'WorkflowInput', {
        ports: JSON.stringify([
          { name: 'alpha', port_type: 'Str' },
          { name: 'beta', port_type: 'Str' },
        ]),
      }),
      makeNode('tpl-a', 'StringTemplate', { num_input: 1, template: '{str0}', strict: true }),
      makeNode('tpl-b', 'StringTemplate', { num_input: 1, template: '{str0}', strict: true }),
    ];
    const edges = [
      makeEdge('e1', 'wf-input', 'tpl-a', 'alpha', 'str0'),
      makeEdge('e2', 'wf-input', 'tpl-b', 'beta', 'str0'),
    ];
    useWorkflowStore.setState({ nodes, edges });

    useWorkflowStore.getState().updateNodeParams('wf-input', {
      ports: JSON.stringify([
        { name: 'beta', port_type: 'Str' },
        { name: 'beta', port_type: 'Str' },
      ]),
    });

    const state = useWorkflowStore.getState();
    const updatedNode = state.nodes.find((node) => node.id === 'wf-input');
    const updatedPorts = JSON.parse(String(updatedNode?.data.params.ports)) as WorkflowPort[];
    const portNames = updatedPorts.map((port) => port.name);

    expect(portNames).toEqual(['beta_2', 'beta']);
    expect(new Set(portNames).size).toBe(portNames.length);
    expect(state.edges.find((edge) => edge.id === 'e1')?.sourceHandle).toBe('beta_2');
    expect(state.edges.find((edge) => edge.id === 'e2')?.sourceHandle).toBe('beta');
  });
});

describe('undo / redo', () => {
  it('undoes addNode and redoes it', () => {
    const { addNode, undo, redo } = useWorkflowStore.getState();

    addNode(makeNode('n1', 'VideoInput'));
    expect(useWorkflowStore.getState().nodes).toHaveLength(1);

    undo();
    expect(useWorkflowStore.getState().nodes).toHaveLength(0);

    redo();
    expect(useWorkflowStore.getState().nodes).toHaveLength(1);
    expect(useWorkflowStore.getState().nodes[0].id).toBe('n1');
  });
});

describe('loadWorkflow', () => {
  it('loads preset JSON correctly', () => {
    useWorkflowStore.getState().loadWorkflow(preset);
    const { nodes, edges } = useWorkflowStore.getState();

    expect(nodes).toHaveLength(5);
    expect(edges).toHaveLength(6);

    const nodeIds = nodes.map((n) => n.id);
    expect(nodeIds).toContain('workflow_input');
    expect(nodeIds).toContain('input');
    expect(nodeIds).toContain('sr');
    expect(nodeIds).toContain('fi');
    expect(nodeIds).toContain('output');

    const srNode = nodes.find((n) => n.id === 'sr');
    expect(srNode?.data.nodeType).toBe('SuperResolution');
    expect(srNode?.data.params.scale).toBe(2);
  });
});

describe('clear', () => {
  it('empties all nodes and edges', () => {
    const { addNode, addEdge, clear } = useWorkflowStore.getState();

    addNode(makeNode('a', 'VideoInput'));
    addNode(makeNode('b', 'SuperResolution', { model_path: 'x', scale: 2, tile_size: 0 }));
    addEdge(makeEdge('e1', 'a', 'b', 'frames', 'frames'));

    expect(useWorkflowStore.getState().nodes).toHaveLength(2);
    expect(useWorkflowStore.getState().edges).toHaveLength(1);

    clear();

    expect(useWorkflowStore.getState().nodes).toHaveLength(0);
    expect(useWorkflowStore.getState().edges).toHaveLength(0);
  });

  it('resets currentFile to null', () => {
    useWorkflowStore.getState().setCurrentFile({
      filename: 'test.json',
      name: 'Test',
      description: 'desc',
    });
    expect(useWorkflowStore.getState().currentFile).not.toBeNull();

    useWorkflowStore.getState().clear();

    expect(useWorkflowStore.getState().currentFile).toBeNull();
  });
});

describe('currentFile', () => {
  it('setCurrentFile stores file info', () => {
    const file = { filename: 'my-flow.json', name: 'My Flow', description: 'A flow' };
    useWorkflowStore.getState().setCurrentFile(file);

    expect(useWorkflowStore.getState().currentFile).toEqual(file);
  });

  it('loadWorkflow resets currentFile', () => {
    useWorkflowStore.getState().setCurrentFile({
      filename: 'old.json',
      name: 'Old',
      description: '',
    });
    expect(useWorkflowStore.getState().currentFile).not.toBeNull();

    useWorkflowStore.getState().loadWorkflow(preset);

    expect(useWorkflowStore.getState().currentFile).toBeNull();
  });
});
