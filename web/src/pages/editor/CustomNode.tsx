import { Handle, type NodeProps, Position, useUpdateNodeInternals } from '@xyflow/react';
import {
  ArrowDownToLine,
  ArrowLeftRight,
  ArrowUpFromLine,
  Braces,
  Download,
  FileVideo,
  Film,
  Globe,
  HardDrive,
  Hash,
  Microscope,
  Palette,
  Plus,
  Radio,
  Scaling,
  Scissors,
  Split,
  Trash2,
  TriangleAlert,
  Workflow,
  X,
} from 'lucide-react';
import { memo, useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { getWorkflowInterface, listModels, type ModelEntry } from '@/api/client';
import { JellyfinLogo } from '@/components/shared/JellyfinLogo';
import { PathAutocomplete } from '@/components/shared/PathAutocomplete';
import { WorkflowPathPicker } from '@/components/shared/WorkflowPathPicker';
import { Checkbox } from '@/components/ui/checkbox';
import { Input } from '@/components/ui/input';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip';
import { getLocalizedNodeTitle } from '@/i18n/node-title';
import { useJobStore } from '@/stores/job-store';
import {
  type PortDescriptor,
  useDescriptor,
  useNodeDefinitions,
} from '@/stores/node-definitions-store';
import { useWorkflowStore } from '@/stores/workflow-store';
import {
  type NodeParamValue,
  type NodeTypeName,
  PORT_COLORS,
  type PortType,
  type WorkflowPort,
} from '@/types';

const ICON_REGISTRY: Record<string, React.ComponentType<{ className?: string }>> = {
  'file-video': FileVideo,
  'microscope': Microscope,
  'film': Film,
  'hard-drive': HardDrive,
  'globe': Globe,
  'radio': Radio,
  'scaling': Scaling,
  'palette': Palette,
  'scissors': Scissors,
  'hash': Hash,
  'tv': JellyfinLogo,
  'arrow-down-to-line': ArrowDownToLine,
  'arrow-up-from-line': ArrowUpFromLine,
  'download': Download,
  'workflow': Workflow,
  'split': Split,
  'braces': Braces,
  'arrow-left-right': ArrowLeftRight,
};

let cachedModels: ModelEntry[] | null = null;
let fetchPromise: Promise<void> | null = null;

function ensureCache(): Promise<void> {
  if (cachedModels !== null) return Promise.resolve();
  if (fetchPromise) return fetchPromise;
  fetchPromise = listModels()
    .then((models) => {
      cachedModels = models;
    })
    .catch(() => {
      cachedModels = [];
    });
  return fetchPromise;
}

// eslint-disable-next-line react-refresh/only-export-components
export function getDefaultBackend(): string {
  return 'cuda';
}

function useModelCache(): { models: ModelEntry[]; defaultBackend: string } {
  const [models, setModels] = useState<ModelEntry[]>(cachedModels ?? []);

  useEffect(() => {
    ensureCache().then(() => {
      setModels(cachedModels ?? []);
    });
  }, []);

  return { models, defaultBackend: 'cuda' };
}

function isStreamPort(port: PortDescriptor): boolean {
  return port.direction === 'stream';
}

function isNvencCodec(codec: unknown): boolean {
  return codec === 'hevc_nvenc' || codec === 'h264_nvenc';
}

function isVisibleVideoOutputPort(
  port: PortDescriptor,
  params: Record<string, NodeParamValue>,
): boolean {
  const nvenc = isNvencCodec(params.codec ?? 'libx265');
  if (port.name === 'crf' || port.name === 'x265_preset') return !nvenc;
  if (port.name === 'cq_value' || port.name === 'nvenc_preset') return nvenc;
  return true;
}

function videoOutputPixelFormats(codec: unknown): readonly string[] {
  if (codec === 'hevc_nvenc') return ['p010le', 'yuv420p'];
  if (codec === 'h264_nvenc' || codec === 'libx264') return ['yuv420p'];
  return ['yuv420p10le', 'yuv420p'];
}

// eslint-disable-next-line react-refresh/only-export-components
export function defaultPixelFormat(codec: string): string {
  if (codec === 'hevc_nvenc') return 'p010le';
  if (codec === 'h264_nvenc' || codec === 'libx264') return 'yuv420p';
  return 'yuv420p10le';
}

function InlineHandle({
  port,
  direction,
  size = 8,
}: {
  port: PortDescriptor;
  direction: 'input' | 'output';
  size?: number;
}) {
  const color = PORT_COLORS[port.port_type as PortType] ?? '#6B7280';

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Handle
          type={direction === 'input' ? 'target' : 'source'}
          position={direction === 'input' ? Position.Left : Position.Right}
          id={port.name}
          className="shrink-0"
          style={{
            position: 'relative',
            top: 'auto',
            left: 'auto',
            right: 'auto',
            transform: 'none',
            margin: 0,
            flexShrink: 0,
            width: size,
            height: size,
            background: color,
            border: `2px solid ${color}`,
            borderRadius: '50%',
          }}
        />
      </TooltipTrigger>
      <TooltipContent side={direction === 'input' ? 'left' : 'right'}>
        <span className="font-mono text-xs">{port.name}</span>
        <span className="ml-1.5 opacity-70 text-xs">{port.port_type}</span>
      </TooltipContent>
    </Tooltip>
  );
}

/** Text for a param field: arrays and objects as JSON, a missing value as the fallback. */
function paramText(value: NodeParamValue | undefined, fallback?: unknown): string {
  const shown = value ?? fallback ?? '';
  return typeof shown === 'object' ? JSON.stringify(shown) : String(shown);
}

/** Params to set when a model is picked; SuperResolution follows the model's native scale. */
// eslint-disable-next-line react-refresh/only-export-components
export function modelSelectionParams(
  nodeType: NodeTypeName,
  modelPath: string,
  model: ModelEntry | undefined,
): Record<string, string | number> {
  if (nodeType === 'SuperResolution' && model?.scale != null) {
    return { model_path: modelPath, scale: model.scale };
  }
  return { model_path: modelPath };
}

function ModelSelector({
  nodeType,
  value,
  onChange,
}: {
  nodeType: NodeTypeName;
  value: string;
  onChange: (path: string, model?: ModelEntry) => void;
}) {
  const { t } = useTranslation('editor');
  const { models } = useModelCache();

  const modelType = nodeType === 'FrameInterpolation' ? 'FrameInterpolation' : 'SuperResolution';
  const filtered = models.filter((m) => m.model_type === modelType);

  if (filtered.length === 0) {
    return (
      <Input
        type="text"
        value={value}
        onChange={(e) => { onChange(e.target.value); }}
        className="h-6 w-[150px] text-[10px] bg-background/50 border-border/50 px-1.5"
        placeholder="models/model.onnx"
      />
    );
  }

  const currentFilename = value.split('/').pop() ?? value;
  const selectedModel = filtered.find((m) => m.filename === currentFilename || value.endsWith(m.filename));

  return (
    <Select
      value={selectedModel?.filename ?? '__custom__'}
      onValueChange={(v) => {
        if (v === '__custom__') return;
        onChange(`models/${v}`, filtered.find((m) => m.filename === v));
      }}
    >
      <SelectTrigger className="h-6 w-[150px] text-[10px] bg-background/50 border-border/50">
        <SelectValue placeholder={t('node.selectModel')} />
      </SelectTrigger>
      <SelectContent>
        {filtered.map((m) => (
          <SelectItem key={m.filename} value={m.filename} className="text-[10px]">
            <span>{m.name}</span>
            {m.scale != null && (
              <span className="ml-1 opacity-60">{m.scale}x</span>
            )}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

function ParamEditorControl({
  nodeId,
  nodeType,
  port,
  value,
  nodeParams,
}: {
  nodeId: string;
  nodeType: NodeTypeName;
  port: PortDescriptor;
  value: NodeParamValue | undefined;
  nodeParams: Record<string, NodeParamValue>;
}) {
  const updateNodeParams = useWorkflowStore((s) => s.updateNodeParams);
  const { defaultBackend } = useModelCache();

  const handleChange = useCallback(
    (newValue: string | number | boolean | undefined) => {
      if (nodeType === 'VideoOutput' && port.name === 'codec' && typeof newValue === 'string') {
        updateNodeParams(nodeId, {
          codec: newValue,
          pixel_format: defaultPixelFormat(newValue),
        });
        return;
      }
      updateNodeParams(nodeId, { [port.name]: newValue });
    },
    [nodeId, nodeType, port.name, updateNodeParams],
  );

  if (port.name === 'model_path') {
    return (
      <ModelSelector
        nodeType={nodeType}
        value={paramText(value)}
        onChange={(path, model) => {
          updateNodeParams(nodeId, modelSelectionParams(nodeType, path, model));
        }}
      />
    );
  }

  const enumOpts = nodeType === 'VideoOutput' && port.name === 'pixel_format'
    ? videoOutputPixelFormats(nodeParams.codec ?? 'libx265')
    : port.enum_options;
  if (enumOpts && enumOpts.length > 0) {
    const resolvedDefault = port.name === 'backend' ? defaultBackend : port.default_value;
    return (
      <Select
        value={paramText(value, resolvedDefault)}
        onValueChange={(v) => { handleChange(v); }}
      >
        <SelectTrigger className="h-6 w-[110px] text-[10px] bg-background/50 border-border/50">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {enumOpts.map((opt) => (
            <SelectItem key={opt} value={opt} className="text-[10px]">
              {opt}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    );
  }

  if (port.port_type === 'Bool') {
    const checked = typeof value === 'boolean' ? value : (port.default_value === true);
    return (
      <Checkbox
        checked={checked}
        onCheckedChange={(v) => { handleChange(!!v); }}
        className="h-3.5 w-3.5"
      />
    );
  }

  if (port.port_type === 'Int' || port.port_type === 'Float') {
    return (
      <Input
        type="number"
        value={paramText(value, port.default_value)}
        step={port.port_type === 'Float' ? '0.01' : '1'}
        onChange={(e) => {
          if (e.target.value === '') {
            handleChange(undefined);
            return;
          }
          const parsed = port.port_type === 'Float'
            ? parseFloat(e.target.value)
            : parseInt(e.target.value, 10);
          if (!Number.isNaN(parsed)) handleChange(parsed);
        }}
        className="h-6 w-[80px] text-[10px] bg-background/50 border-border/50 px-1.5"
      />
    );
  }

  if (port.ui_hint === 'workflow_picker' || port.port_type === 'WorkflowPath') {
    return (
      <WorkflowPathPicker
        value={paramText(value)}
        onChange={handleChange}
      />
    );
  }

  if (port.port_type === 'Path' && port.name !== 'model_path') {
    return (
      <PathAutocomplete
        value={paramText(value, port.default_value)}
        onChange={handleChange}
        className="h-6 w-[150px] text-[10px]"
      />
    );
  }

  if (nodeType === 'Constant' && port.name === 'value' && nodeParams.type === 'Path') {
    return (
      <PathAutocomplete
        value={paramText(value, port.default_value)}
        onChange={handleChange}
        className="h-6 w-[150px] text-[10px]"
      />
    );
  }

  return (
    <Input
      type="text"
      value={paramText(value, port.default_value)}
      onChange={(e) => { handleChange(e.target.value); }}
      className="h-6 w-[130px] text-[10px] bg-background/50 border-border/50 px-1.5"
      placeholder={port.name}
    />
  );
}

const WORKFLOW_PORT_TYPES = ['Int', 'Float', 'Str', 'Bool', 'Path'] as const;

interface DynamicPort {
  name: string;
  port_type: string;
  default_value?: unknown;
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

function toPortType(value: unknown): PortType | undefined {
  if (typeof value !== 'string') return undefined;
  if (!PORT_TYPE_VALUES.has(value as PortType)) return undefined;
  return value as PortType;
}

function resolveDescriptorPortType(
  port: PortDescriptor,
  params?: Record<string, NodeParamValue>,
): PortType | undefined {
  if (port.dynamic_type_param && params) {
    const dynamicType = toPortType(params[port.dynamic_type_param]);
    if (dynamicType) return dynamicType;
  }
  return toPortType(port.port_type);
}

function parseStringTemplateDynamicInputs(
  params: Record<string, NodeParamValue>,
): DynamicPort[] {
  const raw = params.num_input;
  const parsed = typeof raw === 'number' ? raw : Number(raw ?? 0);
  if (!Number.isFinite(parsed)) return [];
  const count = Math.max(0, Math.floor(parsed));
  return Array.from({ length: count }, (_, index) => ({
    name: `str${String(index)}`,
    port_type: 'Str',
  }));
}

function parseDynamicPorts(params: Record<string, NodeParamValue>): DynamicPort[] {
  const raw = params.ports;
  if (!raw || typeof raw !== 'string') return [];
  try {
    return JSON.parse(raw) as DynamicPort[];
  } catch {
    return [];
  }
}

function DynamicPortEditor({
  nodeId,
  nodeType,
  ports,
}: {
  nodeId: string;
  nodeType: string;
  ports: DynamicPort[];
}) {
  const { t } = useTranslation('editor');
  const updateNodeParams = useWorkflowStore((s) => s.updateNodeParams);
  const updateNodeInternals = useUpdateNodeInternals();

  const isInput = nodeType === 'WorkflowInput';
  const direction = isInput ? 'output' : 'input';
  const portsTopologyKey = useMemo(
    () => JSON.stringify(ports.map((port) => ({ name: port.name, port_type: port.port_type }))),
    [ports],
  );

  useEffect(() => {
    void portsTopologyKey;
    updateNodeInternals(nodeId);
  }, [nodeId, portsTopologyKey, updateNodeInternals]);

  const savePorts = useCallback(
    (updated: DynamicPort[]) => {
      updateNodeParams(nodeId, { ports: JSON.stringify(updated) });
    },
    [nodeId, updateNodeParams],
  );

  const addPort = useCallback(() => {
    const idx = ports.length + 1;
    savePorts([...ports, { name: `port_${String(idx)}`, port_type: 'Str' }]);
  }, [ports, savePorts]);

  const removePort = useCallback(
    (index: number) => {
      savePorts(ports.filter((_, i) => i !== index));
    },
    [ports, savePorts],
  );

  const updatePort = useCallback(
    (index: number, field: 'name' | 'port_type', value: string) => {
      const updated = ports.map((p, i) => (i === index ? { ...p, [field]: value } : p));
      savePorts(updated);
    },
    [ports, savePorts],
  );

  return (
    <div className="border-t border-border/40 py-2">
      {ports.map((port, i) => {
        const portDesc: PortDescriptor = {
          name: port.name,
          port_type: port.port_type,
          direction: 'param',
          required: false,
          default_value: null,
          ui_hint: null,
          enum_options: null,
          dynamic_type_param: null,
        };

        return (
          <div
            key={`dyn-port-${String(i)}`}
            className={`nodrag flex items-center gap-2 min-h-8 py-0.5 mb-1.5 ${direction === 'input' ? 'pl-1.5 pr-2' : 'pl-2 pr-1.5'}`}
          >
            {direction === 'input' && <InlineHandle port={portDesc} direction="input" />}
            <Input
              type="text"
              value={port.name}
              onChange={(e) => { updatePort(i, 'name', e.target.value); }}
              className="h-7 flex-1 min-w-0 text-[10px] bg-background/60 border-border/50 px-2"
            />
            <Select
              value={port.port_type}
              onValueChange={(v) => { updatePort(i, 'port_type', v); }}
            >
              <SelectTrigger className="h-7 w-[96px] text-[10px] bg-background/60 border-border/50">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {WORKFLOW_PORT_TYPES.map((t) => (
                  <SelectItem key={t} value={t} className="text-[10px]">{t}</SelectItem>
                ))}
              </SelectContent>
            </Select>
            <button
              type="button"
              onClick={() => { removePort(i); }}
              className="p-1 rounded hover:bg-destructive/20 text-muted-foreground hover:text-destructive shrink-0"
            >
              <Trash2 className="size-3.5" />
            </button>
            {direction === 'output' && <InlineHandle port={portDesc} direction="output" />}
          </div>
        );
      })}
      <button
        type="button"
        onClick={addPort}
        className="flex items-center gap-1 px-3 py-1 text-[11px] text-muted-foreground hover:text-foreground w-full rounded-md hover:bg-secondary/30"
      >
        <Plus className="size-3" />
        <span>{t('node.addPort')}</span>
      </button>
    </div>
  );
}

function parseInterfacePorts(params: Record<string, NodeParamValue>, key: string): WorkflowPort[] {
  const raw = params[key];
  if (!raw || typeof raw !== 'string') return [];
  try {
    return JSON.parse(raw) as WorkflowPort[];
  } catch {
    return [];
  }
}

/** Saved params the node does not declare: shown read-only, each removable. */
function UnsupportedParams({
  nodeId,
  params,
  names,
}: {
  nodeId: string;
  params: Record<string, NodeParamValue>;
  names: string[];
}) {
  const { t } = useTranslation('editor');
  const updateNodeParams = useWorkflowStore((s) => s.updateNodeParams);

  return (
    <div className="border-t border-border/40 px-2 py-1.5">
      <p className="flex items-center gap-1 text-[9px] uppercase tracking-wide text-amber-600 dark:text-amber-400">
        <TriangleAlert className="size-3" />
        {t('node.unsupportedParams')}
      </p>
      <p className="text-[9px] text-muted-foreground">{t('node.unsupportedParamsHint')}</p>
      {names.map((name) => (
        <div key={name} className="nodrag flex items-center gap-2 min-h-6">
          <span className="text-[10px] font-mono text-muted-foreground">{name}</span>
          <span className="flex-1 min-w-0 truncate text-[10px] font-mono text-foreground/80">
            {paramText(params[name])}
          </span>
          <button
            type="button"
            aria-label={t('node.removeParam', { name })}
            onClick={() => { updateNodeParams(nodeId, { [name]: undefined }); }}
            className="p-0.5 rounded hover:bg-destructive/20 text-muted-foreground hover:text-destructive shrink-0"
          >
            <Trash2 className="size-3" />
          </button>
        </div>
      ))}
    </div>
  );
}

/**
 * A node whose type this worker does not define (removed or renamed since
 * the workflow was saved, or definitions failed to load). It is still part
 * of the workflow, so it stays visible, keeps its connections and can be
 * deleted.
 */
function UnsupportedNode({
  id,
  nodeType,
  params,
}: {
  id: string;
  nodeType: string;
  params: Record<string, NodeParamValue>;
}) {
  const { t } = useTranslation('editor');
  const removeNode = useWorkflowStore((s) => s.removeNode);
  const edges = useWorkflowStore((s) => s.edges);
  const definitionsLoaded = useNodeDefinitions((s) => s.descriptors.length > 0);
  const handleFor = (name: string, edgeData: unknown): PortDescriptor => ({
    name,
    port_type:
      edgeData && typeof edgeData === 'object' && 'port_type' in edgeData
        ? String(edgeData.port_type)
        : 'Str',
    direction: 'stream',
    required: false,
    default_value: null,
    ui_hint: null,
    enum_options: null,
    dynamic_type_param: null,
  });
  const inputs = new Map<string, PortDescriptor>();
  const outputs = new Map<string, PortDescriptor>();
  for (const edge of edges) {
    if (edge.target === id && edge.targetHandle) {
      inputs.set(edge.targetHandle, handleFor(edge.targetHandle, edge.data));
    }
    if (edge.source === id && edge.sourceHandle) {
      outputs.set(edge.sourceHandle, handleFor(edge.sourceHandle, edge.data));
    }
  }
  const paramNames = Object.keys(params);

  return (
    <div className="rounded-lg border border-dashed border-destructive/70 shadow-lg overflow-hidden min-w-[220px] bg-card">
      <div className="flex items-center gap-2 px-3 py-2 bg-destructive/15">
        <TriangleAlert className="size-4 text-destructive" />
        <span className="text-xs font-semibold tracking-wide flex-1 font-mono">{nodeType}</span>
        <button
          type="button"
          aria-label={t('node.remove')}
          onClick={(e) => { e.stopPropagation(); removeNode(id); }}
          className="p-0.5 rounded hover:bg-destructive/20"
        >
          <X className="size-3" />
        </button>
      </div>
      <p className="px-3 py-1.5 text-[10px] text-muted-foreground">
        {definitionsLoaded ? t('node.unsupportedType') : t('node.definitionsUnavailable')}
      </p>
      {(inputs.size > 0 || outputs.size > 0) && (
        <div className="flex justify-between gap-6 py-1">
          <div className="flex flex-col gap-0.5">
            {[...inputs.values()].map((port) => (
              <div key={port.name} className="flex items-center gap-1.5 h-[22px] pl-1.5">
                <InlineHandle port={port} direction="input" />
                <span className="text-[10px] text-muted-foreground">{port.name}</span>
              </div>
            ))}
          </div>
          <div className="flex flex-col gap-0.5 items-end">
            {[...outputs.values()].map((port) => (
              <div key={port.name} className="flex items-center gap-1.5 h-[22px] pr-1.5">
                <span className="text-[10px] text-muted-foreground">{port.name}</span>
                <InlineHandle port={port} direction="output" />
              </div>
            ))}
          </div>
        </div>
      )}
      {paramNames.length > 0 && (
        <div className="border-t border-border/40 px-3 py-1">
          {paramNames.map((name) => (
            <div key={name} className="flex gap-2 text-[10px] font-mono">
              <span className="text-muted-foreground">{name}</span>
              <span className="truncate">{paramText(params[name])}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function CustomNodeComponent({ id, data }: NodeProps) {
  const { t } = useTranslation('editor');
  const nodeType = data.nodeType as NodeTypeName;
  const params = data.params as Record<string, NodeParamValue>;
  const desc = useDescriptor(nodeType);
  const runtimePreview = useJobStore((s) => s.runtimePreviewsByNodeId[id]);
  const removeNode = useWorkflowStore((s) => s.removeNode);
  const updateNodeParams = useWorkflowStore((s) => s.updateNodeParams);
  const edges = useWorkflowStore((s) => s.edges);

  const isWorkflowNode = nodeType === 'Workflow';

  const [interfaceInputs, setInterfaceInputs] = useState<WorkflowPort[]>(() =>
    isWorkflowNode ? parseInterfacePorts(params, 'interface_inputs') : [],
  );
  const [interfaceOutputs, setInterfaceOutputs] = useState<WorkflowPort[]>(() =>
    isWorkflowNode ? parseInterfacePorts(params, 'interface_outputs') : [],
  );
  const [interfaceError, setInterfaceError] = useState(false);

  useEffect(() => {
    if (!isWorkflowNode) return;
    const workflowPath = params.workflow_path;

    let cancelled = false;
    const fetchInterface = workflowPath && typeof workflowPath === 'string'
      ? getWorkflowInterface(String(workflowPath))
      : Promise.resolve({ inputs: [] as WorkflowPort[], outputs: [] as WorkflowPort[] });

    fetchInterface
      .then((iface) => {
        if (cancelled) return;
        setInterfaceError(false);
        setInterfaceInputs(iface.inputs);
        setInterfaceOutputs(iface.outputs);
        if (workflowPath && typeof workflowPath === 'string') {
          updateNodeParams(id, {
            interface_inputs: JSON.stringify(iface.inputs),
            interface_outputs: JSON.stringify(iface.outputs),
          });
        }
      })
      .catch(() => {
        if (cancelled) return;
        // Keep the ports last stored with the node: dropping them would drop
        // the handles its connections hang off.
        setInterfaceError(true);
      });

    return () => { cancelled = true; };
  }, [params.workflow_path, isWorkflowNode, id, updateNodeParams]);

  const isParamConnected = useCallback(
    (portName: string) => edges.some((e) => e.target === id && e.targetHandle === portName),
    [edges, id],
  );

  if (!desc) {
    return <UnsupportedNode id={id} nodeType={nodeType} params={params} />;
  }

  const accent = desc.accent_color;
  const IconComp = ICON_REGISTRY[desc.icon];

  const isWorkflowIO = nodeType === 'WorkflowInput' || nodeType === 'WorkflowOutput';
  const dynamicPorts = isWorkflowIO ? parseDynamicPorts(params) : [];

  const streamInputs = desc.inputs.filter((p) => isStreamPort(p));
  const paramInputs = desc.inputs.filter((p) => !isStreamPort(p));
  const stringTemplateInputs = nodeType === 'StringTemplate'
    ? parseStringTemplateDynamicInputs(params).filter(
      (dynamicPort) => !paramInputs.some((port) => port.name === dynamicPort.name),
    ).map<PortDescriptor>((dynamicPort) => ({
      name: dynamicPort.name,
      port_type: dynamicPort.port_type,
      direction: 'param',
      required: false,
      default_value: null,
      ui_hint: null,
      enum_options: null,
      dynamic_type_param: null,
    }))
    : [];
  const visibleParamInputs = nodeType === 'VideoOutput'
    ? paramInputs.filter((port) => isVisibleVideoOutputPort(port, params))
    : paramInputs;
  const effectiveParamInputs = [...visibleParamInputs, ...stringTemplateInputs];
  // Params this worker's node does not declare (renamed or removed since the
  // workflow was saved) are ignored at run time, so show them.
  const knownParamNames = new Set([
    ...desc.inputs.map((port) => port.name),
    ...stringTemplateInputs.map((port) => port.name),
    ...(isWorkflowIO ? ['ports'] : []),
    ...(isWorkflowNode ? ['interface_inputs', 'interface_outputs'] : []),
    ...interfaceInputs.map((port) => port.name),
  ]);
  const unsupportedParams = Object.keys(params).filter((key) => !knownParamNames.has(key));
  const hasStreamPorts = streamInputs.length > 0
    || desc.outputs.length > 0
    || interfaceInputs.length > 0
    || interfaceOutputs.length > 0;

  return (
    <div
      className="group/node rounded-lg border border-border/60 shadow-lg overflow-hidden min-w-[220px]"
      style={{ background: 'var(--card)' }}
    >
      <div
        className="flex items-center gap-2 px-3 py-2"
        style={{ background: accent, color: '#fff' }}
      >
        {IconComp && <IconComp className="size-4" />}
        <span className="text-xs font-semibold tracking-wide flex-1">
          {getLocalizedNodeTitle(t, nodeType, desc.display_name)}
        </span>
        <button
          type="button"
          aria-label={t('node.remove')}
          onClick={(e) => { e.stopPropagation(); removeNode(id); }}
          className="opacity-0 group-hover/node:opacity-100 transition-opacity p-0.5 rounded hover:bg-white/20"
        >
          <X className="size-3" />
        </button>
      </div>

      {isWorkflowNode && interfaceError && (
        <div
          role="alert"
          className="flex items-center gap-1.5 border-b border-border/40 px-2 py-1.5 text-[10px] text-destructive"
        >
          <TriangleAlert className="size-3 shrink-0" />
          <span>{t('node.interfaceLoadFailed', { path: paramText(params.workflow_path) })}</span>
        </div>
      )}

      {hasStreamPorts && (
        <div className="py-1.5">
          <div className="flex justify-between py-0.5 gap-6">
            <div className="flex flex-col gap-0.5 min-w-0">
              {streamInputs.map((port) => (
                <div key={port.name} className="flex items-center gap-1.5 h-[22px] pl-1.5">
                  <InlineHandle port={port} direction="input" />
                  <span className="text-[10px] text-muted-foreground truncate">
                    {port.name}
                  </span>
                </div>
              ))}
              {isWorkflowNode && interfaceInputs.map((wp) => {
                const portDesc: PortDescriptor = {
                  name: wp.name,
                  port_type: wp.port_type,
                  direction: 'stream',
                  required: false,
                  default_value: null,
                  ui_hint: null,
                  enum_options: null,
                  dynamic_type_param: null,
                };
                return (
                  <div key={`wf-in-${wp.name}`} className="flex items-center gap-1.5 h-[22px] pl-1.5">
                    <InlineHandle port={portDesc} direction="input" />
                    <span className="text-[10px] text-muted-foreground truncate">{wp.name}</span>
                  </div>
                );
              })}
            </div>
            <div className="flex flex-col gap-0.5 min-w-0 items-end">
              {desc.outputs.map((port) => {
                const resolvedType = resolveDescriptorPortType(port, params);
                const effectivePort = resolvedType ? { ...port, port_type: resolvedType } : port;
                return (
                  <div key={port.name} className="flex items-center gap-1.5 h-[22px] pr-1.5">
                    <span className="text-[10px] text-muted-foreground truncate">
                      {port.name}
                    </span>
                    <InlineHandle port={effectivePort} direction="output" />
                  </div>
                );
              })}
              {isWorkflowNode && interfaceOutputs.map((wp) => {
                const portDesc: PortDescriptor = {
                  name: wp.name,
                  port_type: wp.port_type,
                  direction: 'stream',
                  required: false,
                  default_value: null,
                  ui_hint: null,
                  enum_options: null,
                  dynamic_type_param: null,
                };
                return (
                  <div key={`wf-out-${wp.name}`} className="flex items-center gap-1.5 h-[22px] pr-1.5">
                    <span className="text-[10px] text-muted-foreground truncate">{wp.name}</span>
                    <InlineHandle port={portDesc} direction="output" />
                  </div>
                );
              })}
            </div>
          </div>
        </div>
      )}

      {effectiveParamInputs.length > 0 && (
        <div className="border-t border-border/40 py-1">
          {effectiveParamInputs.map((port) => {
            const resolvedType = resolveDescriptorPortType(port, params);
            const effectivePort = resolvedType ? { ...port, port_type: resolvedType } : port;
            const connected = isParamConnected(port.name);
            return (
              <div key={port.name} className="flex items-center gap-2 min-h-7 pl-1.5 pr-2">
                <InlineHandle port={effectivePort} direction="input" />
                <span className="text-[10px] leading-none text-muted-foreground truncate flex-1 min-w-0">
                  {port.name}
                </span>
                {connected ? (
                  null
                ) : (
                  <div className="nodrag shrink-0">
                    <ParamEditorControl
                      nodeId={id}
                      nodeType={nodeType}
                      port={effectivePort}
                      value={params[port.name]}
                      nodeParams={params}
                    />
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}

      {isWorkflowIO && (
        <DynamicPortEditor nodeId={id} nodeType={nodeType} ports={dynamicPorts} />
      )}

      {unsupportedParams.length > 0 && (
        <UnsupportedParams nodeId={id} params={params} names={unsupportedParams} />
      )}

      {nodeType === 'Print' && (
        <div className="border-t border-border/40 px-2 py-1.5" data-testid="print-runtime-preview">
          <p className="text-[9px] uppercase tracking-wide text-muted-foreground/80">
            {t('node.runtimePreview')}
          </p>
          <pre
            className="mt-1 max-h-24 overflow-auto rounded border border-border/40 bg-background/60 px-1.5 py-1 text-[10px] leading-4 text-foreground/90 whitespace-pre-wrap break-words"
            data-testid="print-runtime-preview-value"
          >
            {runtimePreview?.value_preview ?? '—'}
          </pre>
          {runtimePreview?.truncated && (
            <p
              className="mt-1 text-[9px] text-muted-foreground"
              data-testid="print-runtime-preview-truncated"
            >
              {t('node.previewTruncated', { max: runtimePreview.preview_max_chars })}
            </p>
          )}
        </div>
      )}
    </div>
  );
}

export const CustomNode = memo(CustomNodeComponent);

// eslint-disable-next-line react-refresh/only-export-components
export function getPortType(
  nodeType: NodeTypeName,
  handleId: string,
  direction: 'input' | 'output',
  nodeParams?: Record<string, NodeParamValue>,
): PortType | undefined {
  if ((nodeType === 'WorkflowInput' || nodeType === 'WorkflowOutput') && nodeParams) {
    const dynPorts = parseDynamicPorts(nodeParams);
    const match = dynPorts.find((p) => p.name === handleId);
    if (match) return match.port_type as PortType;
  }
  if (nodeType === 'Workflow' && nodeParams) {
    const key = direction === 'input' ? 'interface_inputs' : 'interface_outputs';
    const ifacePorts = parseInterfacePorts(nodeParams, key);
    const match = ifacePorts.find((p) => p.name === handleId);
    if (match) return match.port_type as PortType;
  }
  if (nodeType === 'StringTemplate' && direction === 'input' && nodeParams) {
    const match = parseStringTemplateDynamicInputs(nodeParams).find((p) => p.name === handleId);
    if (match) return match.port_type as PortType;
  }
  const descriptors = useNodeDefinitions.getState().descriptors;
  const desc = descriptors.find((d) => d.node_type === nodeType);
  if (!desc) return undefined;
  const ports = direction === 'input' ? desc.inputs : desc.outputs;
  const port = ports.find((p) => p.name === handleId);
  if (!port) return undefined;
  return resolveDescriptorPortType(port, nodeParams);
}
