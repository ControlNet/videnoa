import type { NodeDescriptor } from '../stores/node-definitions-store';
import type {
  AppConfig,
  BatchResponse,
  CreateJobResponse,
  ExtractResponse,
  JobResponse,
  JobWsEvent,
  JobWsNodeDebugValueEvent,
  PerformanceCapabilitiesResponse,
  PerformanceCurrentResponse,
  PerformanceExportResponse,
  PerformanceOverviewResponse,
  Preset,
  ProcessResponse,
  ProgressUpdate,
  Workflow,
  WorkflowInterface,
} from '../types';
import {
  parsePerformanceCapabilitiesResponse,
  parsePerformanceCurrentResponse,
  parsePerformanceExportResponse,
  parsePerformanceOverviewResponse,
} from './performance-validators';

// ─── Model types (backend-specific, not in core type system) ─────────────────

export type ModelType = 'SuperResolution' | 'FrameInterpolation';

export interface ModelEntry {
  name: string;
  model_type: ModelType;
  filename: string;
  url: string | null;
  sha256: string | null;
  scale: number | null;
  input_names: string[];
  output_names: string[];
  normalization_range: [number, number];
  pad_align: number;
  description: string;
  is_fp16: boolean;
  input_format: string;
}

// ─── API Error ───────────────────────────────────────────────────────────────

export class ApiError extends Error {
  status: number;
  body: unknown;

  constructor(status: number, message: string, body?: unknown) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
    this.body = body;
  }
}

// ─── Generic request wrapper ─────────────────────────────────────────────────

async function request<T>(url: string, init?: RequestInit): Promise<T> {
  const resp = await fetch(url, init);
  if (!resp.ok) {
    const text = await resp.text().catch(() => '');
    let message = `HTTP ${String(resp.status)}`;
    let body: unknown;
    try {
      body = JSON.parse(text) as unknown;
      if (typeof body === 'object' && body !== null && 'error' in body) {
        message = String((body as { error: unknown }).error);
      }
    } catch {
      body = { error: text || resp.statusText };
      message = text || resp.statusText;
    }
    throw new ApiError(resp.status, message, body);
  }
  return resp.json() as Promise<T>;
}

async function requestValidated<T>(
  url: string,
  parser: (value: unknown) => T | null,
  init?: RequestInit,
): Promise<T> {
  const payload = await request<unknown>(url, init);
  const parsed = parser(payload);
  if (!parsed) {
    throw new ApiError(500, `Unexpected response payload from ${url}`, payload);
  }
  return parsed;
}

function jsonBody(data: unknown): RequestInit {
  return {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data),
  };
}

// ─── Health ──────────────────────────────────────────────────────────────────

export function healthCheck(): Promise<{ status: string }> {
  return request<{ status: string }>('/api/health');
}

// ─── Nodes ───────────────────────────────────────────────────────────────────

export function listNodes(): Promise<NodeDescriptor[]> {
  return request<NodeDescriptor[]>('/api/nodes');
}

// ─── Models ──────────────────────────────────────────────────────────────────

export function listModels(): Promise<ModelEntry[]> {
  return request<ModelEntry[]>('/api/models');
}

export function getPerformanceCurrent(): Promise<PerformanceCurrentResponse> {
  return requestValidated('/api/performance/current', parsePerformanceCurrentResponse);
}

export function getPerformanceOverview(): Promise<PerformanceOverviewResponse> {
  return requestValidated('/api/performance/overview', parsePerformanceOverviewResponse);
}

export function getPerformanceExport(): Promise<PerformanceExportResponse> {
  return requestValidated('/api/performance/export', parsePerformanceExportResponse);
}

export function getPerformanceCapabilities(): Promise<PerformanceCapabilitiesResponse> {
  return requestValidated('/api/performance/capabilities', parsePerformanceCapabilitiesResponse);
}

// ─── Jobs ────────────────────────────────────────────────────────────────────

export interface SubmitJobOptions {
  workflowName?: string;
}

export function submitJob(
  workflow: Workflow,
  options?: SubmitJobOptions,
): Promise<CreateJobResponse> {
  const payload: {
    workflow: Workflow;
    workflow_name?: string;
  } = { workflow };

  const workflowName = options?.workflowName?.trim();
  if (workflowName) {
    payload.workflow_name = workflowName;
  }

  return request<CreateJobResponse>('/api/jobs', jsonBody(payload));
}

export function submitJobWithParams(
  workflow: Workflow,
  params: Record<string, string | number | boolean>,
  options?: SubmitJobOptions,
): Promise<CreateJobResponse> {
  const payload: {
    workflow: Workflow;
    params: Record<string, string | number | boolean>;
    workflow_name?: string;
  } = { workflow, params };

  const workflowName = options?.workflowName?.trim();
  if (workflowName) {
    payload.workflow_name = workflowName;
  }

  return request<CreateJobResponse>('/api/jobs', jsonBody(payload));
}

export function runByName(
  workflowName: string,
  params?: Record<string, string | number | boolean>,
): Promise<CreateJobResponse> {
  const payload: {
    workflow_name: string;
    params?: Record<string, string | number | boolean>;
  } = {
    workflow_name: workflowName,
  };

  if (params && Object.keys(params).length > 0) {
    payload.params = params;
  }

  return request<CreateJobResponse>('/api/run', jsonBody(payload));
}

export function getJob(id: string): Promise<JobResponse> {
  return request<JobResponse>(`/api/jobs/${id}`);
}

export function listJobs(): Promise<JobResponse[]> {
  return request<JobResponse[]>('/api/jobs');
}

export function rerunJob(id: string): Promise<CreateJobResponse> {
  return request<CreateJobResponse>(`/api/jobs/${id}/rerun`, { method: 'POST' });
}

export async function deleteJobHistory(id: string): Promise<void> {
  const resp = await fetch(`/api/jobs/${id}`, { method: 'DELETE' });
  if (!resp.ok) {
    const text = await resp.text().catch(() => '');
    let message = `HTTP ${String(resp.status)}`;
    let body: unknown;
    try {
      body = JSON.parse(text) as unknown;
      if (typeof body === 'object' && body !== null && 'error' in body) {
        message = String((body as { error: unknown }).error);
      }
    } catch {
      body = { error: text || resp.statusText };
      message = text || resp.statusText;
    }
    throw new ApiError(resp.status, message, body);
  }
}

export function cancelJob(id: string): Promise<void> {
  return deleteJobHistory(id);
}

// ─── WebSocket progress ──────────────────────────────────────────────────────

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

function parseProgressPayload(value: Record<string, unknown>): ProgressUpdate | null {
  const { current_frame, total_frames, fps, eta_seconds } = value;
  if (typeof current_frame !== 'number' || typeof fps !== 'number') {
    return null;
  }
  if (total_frames !== null && total_frames !== undefined && typeof total_frames !== 'number') {
    return null;
  }
  if (eta_seconds !== null && eta_seconds !== undefined && typeof eta_seconds !== 'number') {
    return null;
  }

  return {
    current_frame,
    total_frames: total_frames ?? null,
    fps,
    eta_seconds: eta_seconds ?? null,
  };
}

function parseNodeDebugValuePayload(
  value: Record<string, unknown>,
): JobWsNodeDebugValueEvent | null {
  const { node_id, node_type, value_preview, truncated, preview_max_chars } = value;
  if (
    typeof node_id !== 'string' ||
    typeof node_type !== 'string' ||
    typeof value_preview !== 'string' ||
    typeof truncated !== 'boolean' ||
    typeof preview_max_chars !== 'number'
  ) {
    return null;
  }

  return {
    type: 'node_debug_value',
    node_id,
    node_type,
    value_preview,
    truncated,
    preview_max_chars,
  };
}

function parseJobWsEvent(data: unknown): JobWsEvent | null {
  if (!isRecord(data)) {
    return null;
  }

  const eventType = data.type;
  if (eventType === 'progress') {
    const progress = parseProgressPayload(data);
    return progress ? { type: 'progress', ...progress } : null;
  }

  if (eventType === 'node_debug_value') {
    return parseNodeDebugValuePayload(data);
  }

  const progress = parseProgressPayload(data);
  return progress ? { type: 'progress', ...progress } : null;
}

export function subscribeToProgress(
  jobId: string,
  onProgress: (update: ProgressUpdate) => void,
  onClose: () => void,
  onNodeDebugValue?: (event: JobWsNodeDebugValueEvent) => void,
): { close: () => void } {
  let retries = 0;
  const maxRetries = 3;
  const retryDelay = 2000;
  let ws: WebSocket | null = null;
  let closed = false;

  function connect() {
    const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    ws = new WebSocket(`${proto}//${window.location.host}/api/jobs/${jobId}/ws`);

    ws.onmessage = (event: MessageEvent) => {
      try {
        const parsed = parseJobWsEvent(JSON.parse(String(event.data)));
        if (!parsed) {
          return;
        }

        if (parsed.type === 'progress') {
          onProgress({
            current_frame: parsed.current_frame,
            total_frames: parsed.total_frames,
            fps: parsed.fps,
            eta_seconds: parsed.eta_seconds,
          });
        } else {
          onNodeDebugValue?.(parsed);
        }

        retries = 0; // Reset on successful message
      } catch (err) {
        console.error('Failed to parse job websocket message:', err);
      }
    };

    ws.onclose = () => {
      if (closed) {
        onClose();
        return;
      }
      if (retries < maxRetries) {
        retries++;
        setTimeout(connect, retryDelay);
      } else {
        onClose();
      }
    };

    ws.onerror = () => {
      ws?.close();
    };
  }

  connect();
  return {
    close: () => {
      closed = true;
      ws?.close();
    },
  };
}

// ─── Presets ─────────────────────────────────────────────────────────────────

export function listPresets(): Promise<Preset[]> {
  return request<Preset[]>('/api/presets');
}

export function createPreset(
  name: string,
  description: string,
  workflow: Workflow,
): Promise<Preset> {
  return request<Preset>('/api/presets', jsonBody({ name, description, workflow }));
}

// ─── Workflows ────────────────────────────────────────────────────────────────

export interface WorkflowEntry {
  filename: string;
  name: string;
  description: string;
  workflow: Workflow;
  has_interface: boolean;
}

export function listWorkflows(): Promise<WorkflowEntry[]> {
  return request<WorkflowEntry[]>('/api/workflows');
}

export function saveWorkflow(
  name: string,
  description: string,
  workflow: Workflow,
): Promise<WorkflowEntry> {
  return request<WorkflowEntry>('/api/workflows', jsonBody({ name, description, workflow }));
}

export function getWorkflowInterface(filename: string): Promise<WorkflowInterface> {
  return request<WorkflowInterface>(`/api/workflows/${encodeURIComponent(filename)}/interface`);
}

export async function deleteWorkflow(filename: string): Promise<void> {
  const resp = await fetch(`/api/workflows/${encodeURIComponent(filename)}`, {
    method: 'DELETE',
  });
  if (!resp.ok) {
    const text = await resp.text().catch(() => '');
    throw new ApiError(resp.status, text || resp.statusText);
  }
}

// ─── Batch ───────────────────────────────────────────────────────────────────

export function submitBatch(
  filePaths: string[],
  workflow: Workflow,
): Promise<BatchResponse> {
  return request<BatchResponse>(
    '/api/batch',
    jsonBody({ file_paths: filePaths, workflow }),
  );
}

// ─── Config ──────────────────────────────────────────────────────────────────

export function getConfig(): Promise<AppConfig> {
  return request<AppConfig>('/api/config');
}

export function updateConfig(config: AppConfig): Promise<AppConfig> {
  return request<AppConfig>('/api/config', {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(config),
  });
}

// ─── Preview ─────────────────────────────────────────────────────────────────

export function extractFrames(
  videoPath: string,
  count: number,
): Promise<ExtractResponse> {
  return request<ExtractResponse>(
    '/api/preview/extract',
    jsonBody({ video_path: videoPath, count }),
  );
}

export function processFrame(
  previewId: string,
  frameIndex: number,
  workflow: Workflow,
): Promise<ProcessResponse> {
  return request<ProcessResponse>(
    '/api/preview/process',
    jsonBody({ preview_id: previewId, frame_index: frameIndex, workflow }),
  );
}

// ─── Model inspection ─────────────────────────────────────────────────────────

export interface TensorInfo {
  name: string;
  data_type: string;
  shape: number[];
}

export interface GraphNodeInfo {
  op_type: string;
  name: string;
  inputs: string[];
  outputs: string[];
}

export interface ModelInspection {
  ir_version: number;
  opset_version: number;
  producer_name: string;
  producer_version: string;
  domain: string;
  model_version: number;
  doc_string: string;
  inputs: TensorInfo[];
  outputs: TensorInfo[];
  nodes: GraphNodeInfo[];
  param_count: number;
  op_count: number;
}

export async function inspectModel(filename: string): Promise<ModelInspection> {
  return request<ModelInspection>(`/api/models/${encodeURIComponent(filename)}/inspect`);
}

// ─── Filesystem browsing ──────────────────────────────────────────────────────

export interface FsEntry {
  name: string;
  is_dir: boolean;
  path: string;
}

export async function listDirectory(
  base: string = 'models',
  prefix: string = '',
): Promise<FsEntry[]> {
  const params = new URLSearchParams({ base });
  if (prefix) params.set('prefix', prefix);
  const res = await fetch(`/api/fs/list?${params.toString()}`);
  if (!res.ok) return [];
  return res.json();
}

export async function browseDirectory(path: string = ''): Promise<FsEntry[]> {
  const params = new URLSearchParams();
  if (path) params.set('path', path);
  const res = await fetch(`/api/fs/browse?${params.toString()}`);
  if (!res.ok) return [];
  return res.json();
}
