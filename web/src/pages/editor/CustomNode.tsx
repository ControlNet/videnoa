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

function ModelSelector({
  nodeType,
  value,
  onChange,
}: {
  nodeType: NodeTypeName;
  value: string;
  onChange: (v: string) => void;
}) {
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
        onChange(`models/${v}`);
      }}
    >
      <SelectTrigger className="h-6 w-[150px] text-[10px] bg-background/50 border-border/50">
        <SelectValue placeholder="Select model" />
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
  value: string | number | boolean | undefined;
  nodeParams: Record<string, string | number | boolean>;
}) {
  const updateNodeParams = useWorkflowStore((s) => s.updateNodeParams);
  const { defaultBackend } = useModelCache();

  const handleChange = useCallback(
    (newValue: string | number | boolean) => {
      updateNodeParams(nodeId, { [port.name]: newValue });
    },
    [nodeId, port.name, updateNodeParams],
  );

  if (port.name === 'model_path') {
    return (
      <ModelSelector
        nodeType={nodeType}
        value={String(value ?? '')}
        onChange={handleChange}
      />
    );
  }

  const enumOpts = port.enum_options;
  if (enumOpts && enumOpts.length > 0) {
    const resolvedDefault = port.name === 'backend' ? defaultBackend : port.default_value;
    return (
      <Select
        value={String(value ?? resolvedDefault ?? '')}
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
        value={String(value ?? port.default_value ?? '')}
        step={port.port_type === 'Float' ? '0.01' : '1'}
        onChange={(e) => {
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
        value={String(value ?? '')}
        onChange={handleChange}
      />
    );
  }

  if (port.port_type === 'Path' && port.name !== 'model_path') {
    return (
      <PathAutocomplete
        value={String(value ?? port.default_value ?? '')}
        onChange={handleChange}
        className="h-6 w-[150px] text-[10px]"
      />
    );
  }

  if (nodeType === 'Constant' && port.name === 'value' && nodeParams.type === 'Path') {
    return (
      <PathAutocomplete
        value={String(value ?? port.default_value ?? '')}
        onChange={handleChange}
        className="h-6 w-[150px] text-[10px]"
      />
    );
  }

  return (
    <Input
      type="text"
      value={String(value ?? port.default_value ?? '')}
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
  params?: Record<string, string | number | boolean>,
): PortType | undefined {
  if (port.dynamic_type_param && params) {
    const dynamicType = toPortType(params[port.dynamic_type_param]);
    if (dynamicType) return dynamicType;
  }
  return toPortType(port.port_type);
}

function parseStringTemplateDynamicInputs(
  params: Record<string, string | number | boolean>,
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

function parseDynamicPorts(params: Record<string, string | number | boolean>): DynamicPort[] {
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
        <span>Add Port</span>
      </button>
    </div>
  );
}

function parseInterfacePorts(params: Record<string, string | number | boolean>, key: string): WorkflowPort[] {
  const raw = params[key];
  if (!raw || typeof raw !== 'string') return [];
  try {
    return JSON.parse(raw) as WorkflowPort[];
  } catch {
    return [];
  }
}

function CustomNodeComponent({ id, data }: NodeProps) {
  const { t } = useTranslation('editor');
  const nodeType = data.nodeType as NodeTypeName;
  const params = data.params as Record<string, string | number | boolean>;
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
        setInterfaceInputs([]);
        setInterfaceOutputs([]);
      });

    return () => { cancelled = true; };
  }, [params.workflow_path, isWorkflowNode, id, updateNodeParams]);

  const isParamConnected = useCallback(
    (portName: string) => edges.some((e) => e.target === id && e.targetHandle === portName),
    [edges, id],
  );

  if (!desc) return null;

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
  const effectiveParamInputs = [...paramInputs, ...stringTemplateInputs];
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
          onClick={(e) => { e.stopPropagation(); removeNode(id); }}
          className="opacity-0 group-hover/node:opacity-100 transition-opacity p-0.5 rounded hover:bg-white/20"
        >
          <X className="size-3" />
        </button>
      </div>

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

      {nodeType === 'Print' && (
        <div className="border-t border-border/40 px-2 py-1.5" data-testid="print-runtime-preview">
          <p className="text-[9px] uppercase tracking-wide text-muted-foreground/80">
            Runtime Preview
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
              Truncated to {runtimePreview.preview_max_chars} chars
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
  nodeParams?: Record<string, string | number | boolean>,
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
