import { fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { i18n } from '@/i18n';
import { useNodeDefinitions } from '@/stores/node-definitions-store';
import { useUIStore } from '@/stores/ui-store';
import { NodePalette } from '../NodePalette';

vi.hoisted(() => {
  Object.defineProperty(window, 'matchMedia', {
    writable: true,
    value: vi.fn().mockImplementation((query: string) => ({
      matches: query === '(prefers-color-scheme: dark)',
      media: query,
      onchange: null,
      addListener: vi.fn(),
      removeListener: vi.fn(),
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  });
});

const MOCK_DESCRIPTORS = [
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
  {
    node_type: 'Downloader',
    display_name: 'Downloader',
    category: 'input',
    accent_color: '#0EA5E9',
    icon: 'download',
    inputs: [],
    outputs: [
      { name: 'jellyfin_video', port_type: 'JellyfinVideo', direction: 'stream', required: true, default_value: null, ui_hint: null, enum_options: null, dynamic_type_param: null },
    ],
  },
  {
    node_type: 'JellyfinVideo',
    display_name: 'Jellyfin Video',
    category: 'input',
    accent_color: '#06B6D4',
    icon: 'clapperboard',
    inputs: [],
    outputs: [
      { name: 'frames', port_type: 'VideoFrames', direction: 'stream', required: true, default_value: null, ui_hint: null, enum_options: null, dynamic_type_param: null },
    ],
  },
  {
    node_type: 'PathDivider',
    display_name: 'Path Divider',
    category: 'utility',
    accent_color: '#6366F1',
    icon: 'split',
    inputs: [],
    outputs: [],
  },
  {
    node_type: 'PathJoiner',
    display_name: 'Path Joiner',
    category: 'utility',
    accent_color: '#6366F1',
    icon: 'split',
    inputs: [],
    outputs: [],
  },
  {
    node_type: 'StringReplace',
    display_name: 'String Replace',
    category: 'utility',
    accent_color: '#6366F1',
    icon: 'replace',
    inputs: [],
    outputs: [],
  },
  {
    node_type: 'StringTemplate',
    display_name: 'String Template',
    category: 'utility',
    accent_color: '#6366F1',
    icon: 'braces',
    inputs: [],
    outputs: [],
  },
  {
    node_type: 'TypeConversion',
    display_name: 'Type Conversion',
    category: 'utility',
    accent_color: '#6366F1',
    icon: 'arrow-left-right',
    inputs: [],
    outputs: [],
  },
  {
    node_type: 'HttpRequest',
    display_name: 'HTTP Request',
    category: 'utility',
    accent_color: '#6366F1',
    icon: 'globe',
    inputs: [],
    outputs: [],
  },
  {
    node_type: 'Print',
    display_name: 'Print',
    category: 'utility',
    accent_color: '#6366F1',
    icon: 'hash',
    inputs: [],
    outputs: [],
  },
  {
    node_type: 'CustomBackendNode',
    display_name: 'Backend Node Name',
    category: 'utility',
    accent_color: '#10B981',
    icon: 'hash',
    inputs: [],
    outputs: [],
  },
  {
    node_type: 'Resize',
    display_name: 'Backend Resize Name',
    category: 'processing',
    accent_color: '#22C55E',
    icon: 'scaling',
    inputs: [],
    outputs: [],
  },
  {
    node_type: 'Rescale',
    display_name: 'Backend Rescale Name',
    category: 'processing',
    accent_color: '#84CC16',
    icon: 'scaling',
    inputs: [],
    outputs: [],
  },
  {
    node_type: 'ColorSpace',
    display_name: 'Backend ColorSpace Name',
    category: 'processing',
    accent_color: '#F59E0B',
    icon: 'palette',
    inputs: [],
    outputs: [],
  },
  {
    node_type: 'SceneDetect',
    display_name: 'Backend SceneDetect Name',
    category: 'processing',
    accent_color: '#EF4444',
    icon: 'scissors',
    inputs: [],
    outputs: [],
  },
  {
    node_type: 'StreamOutput',
    display_name: 'Backend StreamOutput Name',
    category: 'output',
    accent_color: '#3B82F6',
    icon: 'arrow-up-from-line',
    inputs: [],
    outputs: [],
  },
  {
    node_type: 'Constant',
    display_name: 'Backend Constant Name',
    category: 'utility',
    accent_color: '#14B8A6',
    icon: 'hash',
    inputs: [],
    outputs: [],
  },
];

beforeEach(async () => {
  await i18n.changeLanguage('en');
  useNodeDefinitions.setState({ descriptors: MOCK_DESCRIPTORS, loading: false, error: null });
  useUIStore.setState({ sidebarCollapsed: false });
});

describe('NodePalette', () => {
  it('renders node names when expanded', () => {
    render(<NodePalette />);
    expect(screen.getByText('Video Input')).toBeInTheDocument();
    expect(screen.getByText('Super Resolution')).toBeInTheDocument();
    expect(screen.getByText('Downloader')).toBeInTheDocument();
    expect(screen.getByText('Jellyfin Video')).toBeInTheDocument();
    expect(screen.getByText('Path Divider')).toBeInTheDocument();
    expect(screen.getByText('Path Joiner')).toBeInTheDocument();
    expect(screen.getByText('String Replace')).toBeInTheDocument();
    expect(screen.getByText('String Template')).toBeInTheDocument();
    expect(screen.getByText('Type Conversion')).toBeInTheDocument();
    expect(screen.getByText('HTTP Request')).toBeInTheDocument();
    expect(screen.getByText('Print')).toBeInTheDocument();
    expect(screen.getByText('Backend Node Name')).toBeInTheDocument();
    expect(screen.getByText('Resize')).toBeInTheDocument();
    expect(screen.getByText('Rescale')).toBeInTheDocument();
    expect(screen.getByText('Color Space')).toBeInTheDocument();
    expect(screen.getByText('Scene Detect')).toBeInTheDocument();
    expect(screen.getByText('Stream Output')).toBeInTheDocument();
    expect(screen.getByText('Constant')).toBeInTheDocument();
  });

  it('localizes mapped node titles and falls back to display_name for unknown types', async () => {
    await i18n.changeLanguage('zh-CN');
    render(<NodePalette />);

    expect(screen.getByText('视频输入')).toBeInTheDocument();
    expect(screen.getByText('超分辨率')).toBeInTheDocument();
    expect(screen.getByText('下载器')).toBeInTheDocument();
    expect(screen.getByText('Jellyfin 视频')).toBeInTheDocument();
    expect(screen.getByText('路径拆分')).toBeInTheDocument();
    expect(screen.getByText('路径拼接')).toBeInTheDocument();
    expect(screen.getByText('字符串替换')).toBeInTheDocument();
    expect(screen.getByText('字符串模板')).toBeInTheDocument();
    expect(screen.getByText('类型转换')).toBeInTheDocument();
    expect(screen.getByText('HTTP 请求')).toBeInTheDocument();
    expect(screen.getByText('打印')).toBeInTheDocument();
    expect(screen.getByText('调整尺寸')).toBeInTheDocument();
    expect(screen.getByText('重缩放')).toBeInTheDocument();
    expect(screen.getByText('色彩空间')).toBeInTheDocument();
    expect(screen.getByText('场景检测')).toBeInTheDocument();
    expect(screen.getByText('流输出')).toBeInTheDocument();
    expect(screen.getByText('常量')).toBeInTheDocument();
    expect(screen.getByText('Backend Node Name')).toBeInTheDocument();
  });

  it('renders download icon for Downloader', () => {
    render(<NodePalette />);
    const downloaderButton = screen.getByText('Downloader').closest('button');
    expect(downloaderButton).toBeTruthy();
    const icon = downloaderButton?.querySelector('svg.lucide-download');
    expect(icon).toBeInTheDocument();
  });

  it('renders utility icons for new node types', () => {
    render(<NodePalette />);

    const pathDividerButton = screen.getByText('Path Divider').closest('button');
    expect(pathDividerButton?.querySelector('svg.lucide-split')).toBeInTheDocument();

    const pathJoinerButton = screen.getByText('Path Joiner').closest('button');
    expect(pathJoinerButton?.querySelector('svg.lucide-split')).toBeInTheDocument();

    const stringTemplateButton = screen.getByText('String Template').closest('button');
    expect(stringTemplateButton?.querySelector('svg.lucide-braces')).toBeInTheDocument();

    const stringReplaceButton = screen.getByText('String Replace').closest('button');
    expect(stringReplaceButton?.querySelector('svg.lucide-replace')).toBeInTheDocument();

    const typeConversionButton = screen.getByText('Type Conversion').closest('button');
    expect(typeConversionButton?.querySelector('svg.lucide-arrow-left-right')).toBeInTheDocument();

    const httpRequestButton = screen.getByText('HTTP Request').closest('button');
    expect(httpRequestButton?.querySelector('svg.lucide-globe')).toBeInTheDocument();

    const printButton = screen.getByText('Print').closest('button');
    expect(printButton?.querySelector('svg.lucide-hash')).toBeInTheDocument();
  });

  it('renders category labels', () => {
    render(<NodePalette />);
    expect(screen.getByText('input')).toBeInTheDocument();
    expect(screen.getByText('processing')).toBeInTheDocument();
  });

  it('does not show legacy count badges', () => {
    render(<NodePalette />);
    expect(screen.queryAllByText('2')).toHaveLength(0);
  });

  it('collapses when sidebarCollapsed is true', () => {
    useUIStore.setState({ sidebarCollapsed: true });
    render(<NodePalette />);
    expect(screen.queryByText('Video Input')).not.toBeInTheDocument();
    expect(screen.queryByText('Super Resolution')).not.toBeInTheDocument();
    expect(screen.queryByText('Nodes')).not.toBeInTheDocument();
  });

  it('node items are draggable', () => {
    render(<NodePalette />);
    const nodeButtons = screen.getAllByRole('button').filter(
      (btn) => btn.getAttribute('draggable') === 'true',
    );
    expect(nodeButtons.length).toBe(18);
  });

  it('writes both reactflow and text payload on drag start', () => {
    render(<NodePalette />);

    const source = screen.getByText('Video Input').closest('button');
    expect(source).toBeTruthy();

    const setData = vi.fn();
    fireEvent.dragStart(source as Element, {
      dataTransfer: {
        setData,
        effectAllowed: 'none',
      },
    });

    expect(setData).toHaveBeenCalledWith('application/reactflow', 'VideoInput');
    expect(setData).toHaveBeenCalledWith('text/plain', 'VideoInput');
  });
});
