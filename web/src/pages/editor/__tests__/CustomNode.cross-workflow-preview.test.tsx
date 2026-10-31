import { act, render, screen } from '@testing-library/react';
import type React from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import * as api from '@/api/client';
import { TooltipProvider } from '@/components/ui/tooltip';
import { useJobStore } from '@/stores/job-store';
import { useNodeDefinitions } from '@/stores/node-definitions-store';
import { useWorkflowStore } from '@/stores/workflow-store';
import type { Workflow } from '@/types';
import { CustomNode } from '../CustomNode';

vi.mock('@xyflow/react', () => ({
  Handle: ({ id, type }: { id: string; type: string }) => (
    <div data-testid={`handle-${type}-${id}`} />
  ),
  Position: { Left: 'left', Right: 'right' },
}));

vi.mock('@/api/client', () => ({
  listModels: vi.fn().mockResolvedValue([]),
  getWorkflowInterface: vi.fn().mockResolvedValue({ inputs: [], outputs: [] }),
  subscribeToProgress: vi.fn(),
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

// Workflow B is unrelated to the running job but reuses the node id "print-1".
const WORKFLOW_B: Workflow = {
  nodes: [{ id: 'print-1', node_type: 'Print', params: {} }],
  connections: [],
};

type DebugCallback = Parameters<typeof api.subscribeToProgress>[3];

beforeEach(() => {
  vi.clearAllMocks();
  useNodeDefinitions.setState({
    descriptors: [PRINT_DESCRIPTOR],
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
    submittedJobId: null,
  });
  useJobStore.setState({
    jobs: [],
    activeJobId: null,
    activeProgress: null,
    runtimePreviewsByNodeId: {},
    runtimePreviewsByJobId: {},
    wsCleanup: null,
  });
});

// Workflow A was submitted from this canvas as job-A, which the editor then
// tracks, and job-A printed a value for print-1.
async function renderPrintNodeOfRunningCanvasJob(): Promise<void> {
  let emitDebug: DebugCallback | undefined;
  vi.mocked(api.subscribeToProgress).mockImplementation(
    (_jobId, _onProgress, _onClose, onDebug) => {
      emitDebug = onDebug;
      return { close: vi.fn() };
    },
  );
  useWorkflowStore.getState().setSubmittedJobId('job-A');
  useJobStore.getState().subscribeToJob('job-A');
  emitDebug?.({
    type: 'node_debug_value',
    node_id: 'print-1',
    node_type: 'Print',
    value_preview: 'value-from-workflow-A',
    truncated: false,
    preview_max_chars: 512,
  });

  await act(async () => {
    render(
      <TooltipProvider>
        <CustomNode
          {...({
            id: 'print-1',
            data: { nodeType: 'Print', params: {} },
          } as unknown as React.ComponentProps<typeof CustomNode>)}
        />
      </TooltipProvider>,
    );
  });
  expect(screen.getByTestId('print-runtime-preview-value')).toHaveTextContent(
    'value-from-workflow-A',
  );
}

describe('Print runtime preview isolation across workflows', () => {
  it("does not show job A's Print value after workflow B is loaded", async () => {
    await renderPrintNodeOfRunningCanvasJob();

    act(() => {
      useWorkflowStore.getState().loadWorkflow(WORKFLOW_B);
    });

    expect(screen.getByTestId('print-runtime-preview-value')).toHaveTextContent('—');
  });

  it("does not show job A's Print value after a new workflow is started", async () => {
    await renderPrintNodeOfRunningCanvasJob();

    act(() => {
      useWorkflowStore.getState().clear();
    });

    expect(screen.getByTestId('print-runtime-preview-value')).toHaveTextContent('—');
  });
});
