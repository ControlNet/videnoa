import { act, render, screen } from '@testing-library/react';
import type React from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { TooltipProvider } from '@/components/ui/tooltip';
import { useJobStore } from '@/stores/job-store';
import { useNodeDefinitions } from '@/stores/node-definitions-store';
import { useWorkflowStore } from '@/stores/workflow-store';
import { CustomNode, defaultPixelFormat } from '../CustomNode';

vi.mock('@xyflow/react', () => ({
  Handle: ({ id, type }: { id: string; type: string }) => (
    <div data-testid={`handle-${type}-${id}`} />
  ),
  Position: {
    Left: 'left',
    Right: 'right',
  },
}));

vi.mock('@/api/client', () => ({
  listModels: vi.fn().mockResolvedValue([]),
  getWorkflowInterface: vi.fn().mockResolvedValue({ inputs: [], outputs: [] }),
}));

vi.mock('@/components/shared/PathAutocomplete', () => ({
  PathAutocomplete: ({ value }: { value: string }) => (
    <div data-testid="path-autocomplete" data-value={value} />
  ),
}));

const PRINT_DESCRIPTOR = {
  node_type: 'Print',
  display_name: 'Print',
  category: 'utility',
  accent_color: '#6366F1',
  icon: 'hash',
  inputs: [
    {
      name: 'value',
      port_type: 'Str',
      direction: 'param',
      required: false,
      default_value: null,
      ui_hint: null,
      enum_options: null,
      dynamic_type_param: null,
    },
  ],
  outputs: [],
};

const VIDEO_INPUT_DESCRIPTOR = {
  node_type: 'VideoInput',
  display_name: 'Video Input',
  category: 'input',
  accent_color: '#8B5CF6',
  icon: 'file-video',
  inputs: [],
  outputs: [],
};

const CONSTANT_DESCRIPTOR = {
  node_type: 'Constant',
  display_name: 'Constant',
  category: 'utility',
  accent_color: '#6366F1',
  icon: 'hash',
  inputs: [
    {
      name: 'type',
      port_type: 'Str',
      direction: 'param',
      required: false,
      default_value: 'Int',
      ui_hint: null,
      enum_options: ['Int', 'Float', 'Str', 'Bool', 'Path'],
      dynamic_type_param: null,
    },
    {
      name: 'value',
      port_type: 'Str',
      direction: 'param',
      required: false,
      default_value: '0',
      ui_hint: null,
      enum_options: null,
      dynamic_type_param: null,
    },
  ],
  outputs: [],
};

const VIDEO_OUTPUT_DESCRIPTOR = {
  node_type: 'VideoOutput',
  display_name: 'Video Output',
  category: 'output',
  accent_color: '#10B981',
  icon: 'hard-drive',
  inputs: [
    { name: 'codec', port_type: 'Str', direction: 'param', required: false, default_value: 'libx265', ui_hint: null, enum_options: ['libx265', 'libx264', 'hevc_nvenc', 'h264_nvenc'], dynamic_type_param: null },
    { name: 'crf', port_type: 'Int', direction: 'param', required: false, default_value: 18, ui_hint: null, enum_options: null, dynamic_type_param: null },
    { name: 'cq_value', port_type: 'Int', direction: 'param', required: false, default_value: 20, ui_hint: null, enum_options: null, dynamic_type_param: null },
    { name: 'nvenc_preset', port_type: 'Str', direction: 'param', required: false, default_value: 'p4', ui_hint: null, enum_options: ['p1', 'p2', 'p3', 'p4', 'p5', 'p6', 'p7'], dynamic_type_param: null },
    { name: 'x265_preset', port_type: 'Str', direction: 'param', required: false, default_value: 'medium', ui_hint: null, enum_options: ['ultrafast', 'fast', 'medium', 'slow', 'veryslow'], dynamic_type_param: null },
    { name: 'pixel_format', port_type: 'Str', direction: 'param', required: false, default_value: 'yuv420p10le', ui_hint: null, enum_options: ['yuv420p10le', 'p010le', 'yuv420p'], dynamic_type_param: null },
  ],
  outputs: [],
};

async function renderNode(
  nodeType: string,
  id: string,
  params: Record<string, string | number | boolean> = {},
): Promise<void> {
  await act(async () => {
    render(
      <TooltipProvider>
        <CustomNode
          {...({
            id,
            data: {
              nodeType,
              params,
            },
          } as unknown as React.ComponentProps<typeof CustomNode>)}
        />
      </TooltipProvider>,
    );
  });
}

beforeEach(() => {
  useNodeDefinitions.setState({
    descriptors: [PRINT_DESCRIPTOR, VIDEO_INPUT_DESCRIPTOR, CONSTANT_DESCRIPTOR, VIDEO_OUTPUT_DESCRIPTOR],
    loading: false,
    error: null,
    fetch: vi.fn().mockResolvedValue(undefined),
  });

  useWorkflowStore.setState({
    nodes: [],
    edges: [],
    past: [],
    future: [],
    currentFile: null,
  });

  useJobStore.setState({
    jobs: [],
    activeJobId: null,
    activeProgress: null,
    runtimePreviewsByNodeId: {},
    wsCleanup: null,
  });
});

describe('CustomNode Print runtime preview', () => {
  it('renders Print runtime preview panel with empty fallback by default', async () => {
    await renderNode('Print', 'print-1');

    expect(screen.getByTestId('print-runtime-preview')).toBeInTheDocument();
    expect(screen.getByTestId('print-runtime-preview-value')).toHaveTextContent('—');
    expect(screen.queryByTestId('print-runtime-preview-truncated')).not.toBeInTheDocument();
  });

  it('does not render runtime preview panel for non-Print nodes', async () => {
    useJobStore.setState({
      runtimePreviewsByNodeId: {
        'video-1': {
          node_id: 'video-1',
          node_type: 'Print',
          value_preview: 'should not render',
          truncated: false,
          preview_max_chars: 512,
          updated_at_ms: 1,
        },
      },
    });

    await renderNode('VideoInput', 'video-1');

    expect(screen.queryByTestId('print-runtime-preview')).not.toBeInTheDocument();
  });

  it('reads latest runtime preview by node id and shows truncation hint', async () => {
    useJobStore.setState({
      runtimePreviewsByNodeId: {
        'print-1': {
          node_id: 'print-1',
          node_type: 'Print',
          value_preview: 'first preview',
          truncated: false,
          preview_max_chars: 512,
          updated_at_ms: 1,
        },
      },
    });

    await renderNode('Print', 'print-1');

    expect(screen.getByTestId('print-runtime-preview-value')).toHaveTextContent('first preview');
    expect(screen.queryByTestId('print-runtime-preview-truncated')).not.toBeInTheDocument();

    act(() => {
      useJobStore.setState({
        runtimePreviewsByNodeId: {
          'print-1': {
            node_id: 'print-1',
            node_type: 'Print',
            value_preview: 'second preview',
            truncated: true,
            preview_max_chars: 128,
            updated_at_ms: 2,
          },
        },
      });
    });

    expect(screen.getByTestId('print-runtime-preview-value')).toHaveTextContent('second preview');
    expect(screen.getByTestId('print-runtime-preview-truncated')).toHaveTextContent('Truncated to 128 chars');
  });
});

describe('CustomNode Constant value editor', () => {
  it('uses path autocomplete when Constant type is Path', async () => {
    await renderNode('Constant', 'constant-path', {
      type: 'Path',
      value: '/tmp/video.mkv',
    });

    expect(screen.getByTestId('path-autocomplete')).toBeInTheDocument();
  });

  it('keeps text input when Constant type is Str', async () => {
    await renderNode('Constant', 'constant-str', {
      type: 'Str',
      value: 'plain text',
    });

    expect(screen.queryByTestId('path-autocomplete')).not.toBeInTheDocument();
    expect(screen.getByPlaceholderText('value')).toBeInTheDocument();
  });
});

describe('CustomNode VideoOutput encoder controls', () => {
  it('shows CRF and software preset for libx265', async () => {
    await renderNode('VideoOutput', 'video-output-software', { codec: 'libx265' });

    expect(screen.getByText('crf')).toBeInTheDocument();
    expect(screen.getByText('x265_preset')).toBeInTheDocument();
    expect(screen.queryByText('cq_value')).not.toBeInTheDocument();
    expect(screen.queryByText('nvenc_preset')).not.toBeInTheDocument();
  });

  it('shows CQ and NVENC preset for hevc_nvenc', async () => {
    await renderNode('VideoOutput', 'video-output-nvenc', { codec: 'hevc_nvenc' });

    expect(screen.getByText('cq_value')).toBeInTheDocument();
    expect(screen.getByText('nvenc_preset')).toBeInTheDocument();
    expect(screen.queryByText('crf')).not.toBeInTheDocument();
    expect(screen.queryByText('x265_preset')).not.toBeInTheDocument();
  });

  it('maps codecs to compatible default pixel formats', () => {
    expect(defaultPixelFormat('libx265')).toBe('yuv420p10le');
    expect(defaultPixelFormat('libx264')).toBe('yuv420p');
    expect(defaultPixelFormat('hevc_nvenc')).toBe('p010le');
    expect(defaultPixelFormat('h264_nvenc')).toBe('yuv420p');
  });
});
