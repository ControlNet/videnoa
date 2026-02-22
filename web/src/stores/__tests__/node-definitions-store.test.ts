import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { useNodeDefinitions } from '../node-definitions-store';
import type { NodeDescriptor } from '../node-definitions-store';

const MOCK_DESCRIPTORS: NodeDescriptor[] = [
  {
    node_type: 'VideoInput',
    display_name: 'Video Input',
    category: 'input',
    accent_color: '#8B5CF6',
    icon: 'file-video',
    inputs: [],
    outputs: [
      { name: 'frames', port_type: 'VideoFrames', direction: 'stream', required: true, default_value: null, ui_hint: null, enum_options: null, dynamic_type_param: null },
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
    ],
    outputs: [
      { name: 'frames', port_type: 'VideoFrames', direction: 'stream', required: true, default_value: null, ui_hint: null, enum_options: null, dynamic_type_param: null },
    ],
  },
];

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });
}

beforeEach(() => {
  vi.stubGlobal('fetch', vi.fn());
  useNodeDefinitions.setState({
    descriptors: [],
    loading: false,
    error: null,
  });
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe('initial state', () => {
  it('has empty descriptors and no error', () => {
    const state = useNodeDefinitions.getState();
    expect(state.descriptors).toEqual([]);
    expect(state.loading).toBe(false);
    expect(state.error).toBeNull();
  });
});

describe('fetch', () => {
  it('populates descriptors from API', async () => {
    vi.mocked(fetch).mockResolvedValueOnce(jsonResponse(MOCK_DESCRIPTORS));

    await useNodeDefinitions.getState().fetch();

    const state = useNodeDefinitions.getState();
    expect(state.descriptors).toHaveLength(2);
    expect(state.descriptors[0].node_type).toBe('VideoInput');
    expect(state.descriptors[1].node_type).toBe('SuperResolution');
    expect(state.loading).toBe(false);
    expect(state.error).toBeNull();
  });

  it('skips re-fetch when descriptors already loaded', async () => {
    useNodeDefinitions.setState({ descriptors: MOCK_DESCRIPTORS });

    await useNodeDefinitions.getState().fetch();

    expect(fetch).not.toHaveBeenCalled();
  });

  it('sets error on non-ok response', async () => {
    vi.mocked(fetch).mockResolvedValueOnce(
      new Response('Not Found', { status: 404, statusText: 'Not Found' }),
    );

    await useNodeDefinitions.getState().fetch();

    const state = useNodeDefinitions.getState();
    expect(state.error).toContain('404');
    expect(state.loading).toBe(false);
    expect(state.descriptors).toEqual([]);
  });

  it('sets error on network failure', async () => {
    vi.mocked(fetch).mockRejectedValueOnce(new TypeError('Failed to fetch'));

    await useNodeDefinitions.getState().fetch();

    const state = useNodeDefinitions.getState();
    expect(state.error).toContain('Failed to fetch');
    expect(state.loading).toBe(false);
  });

  it('sets loading true while fetching', async () => {
    let resolvePromise: (value: Response) => void;
    const pending = new Promise<Response>((resolve) => {
      resolvePromise = resolve;
    });
    vi.mocked(fetch).mockReturnValueOnce(pending);

    const fetchPromise = useNodeDefinitions.getState().fetch();
    expect(useNodeDefinitions.getState().loading).toBe(true);

    resolvePromise!(jsonResponse(MOCK_DESCRIPTORS));
    await fetchPromise;

    expect(useNodeDefinitions.getState().loading).toBe(false);
  });
});

describe('selectors via getState', () => {
  beforeEach(() => {
    useNodeDefinitions.setState({ descriptors: MOCK_DESCRIPTORS });
  });

  it('finds descriptor by node_type', () => {
    const state = useNodeDefinitions.getState();
    const found = state.descriptors.find((d) => d.node_type === 'VideoInput');
    expect(found).toBeDefined();
    expect(found!.display_name).toBe('Video Input');
  });

  it('returns undefined for unknown node_type', () => {
    const state = useNodeDefinitions.getState();
    const found = state.descriptors.find((d) => d.node_type === 'NonExistent');
    expect(found).toBeUndefined();
  });

  it('returns all descriptors', () => {
    const state = useNodeDefinitions.getState();
    expect(state.descriptors).toHaveLength(2);
    expect(state.descriptors.map((d) => d.node_type)).toEqual(['VideoInput', 'SuperResolution']);
  });
});
