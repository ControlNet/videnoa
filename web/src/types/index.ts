// ─── Port Types ──────────────────────────────────────────────────────────────

export type PortType =
  | 'VideoFrames'
  | 'Metadata'
  | 'Model'
  | 'Int'
  | 'Float'
  | 'Str'
  | 'Bool'
  | 'Path'
  | 'WorkflowPath';

export const PORT_COLORS: Record<PortType, string> = {
  VideoFrames: '#8B5CF6',
  Metadata: '#3B82F6',
  Model: '#F97316',
  Int: '#22C55E',
  Float: '#06B6D4',
  Str: '#EAB308',
  Bool: '#EF4444',
  Path: '#6B7280',
  WorkflowPath: '#A855F7',
};

// ─── Port & Node Definitions ─────────────────────────────────────────────────

export interface PortDefinition {
  name: string;
  type: PortType;
  required?: boolean;
  default?: string | number | boolean;
}

export interface NodeDefinition {
  inputs: PortDefinition[];
  outputs: PortDefinition[];
}

export type NodeTypeName = string;

// ─── Workflow JSON (matches backend PipelineGraph serde) ─────────────────────

export interface WorkflowNode {
  id: string;
  node_type: NodeTypeName;
  params: Record<string, unknown>;
}

export interface WorkflowConnection {
  from_node: string;
  from_port: string;
  to_node: string;
  to_port: string;
  port_type: PortType;
}

export interface WorkflowPort {
  name: string;
  port_type: string;
  default_value?: unknown;
}

export interface WorkflowInterface {
  inputs: WorkflowPort[];
  outputs: WorkflowPort[];
}

export interface Workflow {
  nodes: WorkflowNode[];
  connections: WorkflowConnection[];
  interface?: WorkflowInterface;
}

// ─── React Flow node data ────────────────────────────────────────────────────

export interface PipelineNodeData {
  nodeType: NodeTypeName;
  params: Record<string, string | number | boolean>;
  [key: string]: unknown;
}

// ─── Job types ───────────────────────────────────────────────────────────────

export type JobStatus = 'queued' | 'running' | 'completed' | 'failed' | 'cancelled';

export interface ProgressUpdate {
  current_frame: number;
  total_frames: number | null;
  fps: number;
  eta_seconds: number | null;
}

export interface JobWsProgressEvent extends ProgressUpdate {
  type: 'progress';
}

export interface JobWsNodeDebugValueEvent {
  type: 'node_debug_value';
  node_id: string;
  node_type: string;
  value_preview: string;
  truncated: boolean;
  preview_max_chars: number;
}

export type JobWsEvent = JobWsProgressEvent | JobWsNodeDebugValueEvent;

export interface NodeRuntimePreview {
  node_id: string;
  node_type: string;
  value_preview: string;
  truncated: boolean;
  preview_max_chars: number;
  updated_at_ms: number;
}

export interface Job {
  id: string;
  status: JobStatus;
  progress: ProgressUpdate | null;
  created_at: string;
  started_at: string | null;
  completed_at: string | null;
  error: string | null;
  workflow_name?: string;
  workflow_source?: string;
  params?: Record<string, unknown> | null;
  rerun_of_job_id?: string | null;
  duration_ms?: number | null;
}

// ─── API response types (matching backend JSON) ─────────────────────────────

export interface CreateJobResponse {
  id: string;
  status: string;
  created_at: string;
}

export interface JobResponse {
  id: string;
  status: JobStatus;
  created_at: string;
  started_at: string | null;
  completed_at: string | null;
  progress: ProgressUpdate | null;
  error: string | null;
  workflow_name: string;
  workflow_source: string;
  params: Record<string, unknown> | null;
  rerun_of_job_id: string | null;
  duration_ms: number | null;
}

export interface Preset {
  id: string;
  name: string;
  description: string;
  workflow: Workflow;
}

export interface BatchResponse {
  job_ids: string[];
  total: number;
}

export interface AppConfig {
  paths: {
    models_dir: string;
    trt_cache_dir: string;
    presets_dir: string;
    workflows_dir: string;
  };
  server: {
    port: number;
    host: string;
  };
  locale: string;
  performance: {
    profiling_enabled: boolean;
  };
}

// ─── Preview / Before-After Comparison ───────────────────────────────────────

export interface FrameInfo {
  index: number;
  url: string;
}

export interface ExtractResponse {
  preview_id: string;
  frames: FrameInfo[];
}

export interface ProcessResponse {
  processed_url: string;
}

export type PerformanceStatus = 'disabled' | 'enabled' | 'degraded' | 'partial';

export interface PerformanceEnvelope {
  status: PerformanceStatus;
  enabled: boolean;
  reason: string;
  message: string;
}

export type PerformanceMetrics = Record<string, number | null>;

export interface PerformanceCurrentResponse extends PerformanceEnvelope {
  metrics: PerformanceMetrics | null;
}

export interface PerformanceOverviewResponse extends PerformanceEnvelope {
  metrics: PerformanceMetrics | null;
}

export interface PerformanceSeriesPoint {
  timestamp_ms: number;
  metrics: PerformanceMetrics;
}

export interface PerformanceExportResponse extends PerformanceEnvelope {
  series: PerformanceSeriesPoint[];
}

export interface PerformanceCapabilitiesResponse extends PerformanceEnvelope {
  supported_statuses: PerformanceStatus[];
}
