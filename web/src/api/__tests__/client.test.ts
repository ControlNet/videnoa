import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Workflow } from '../../types';
import {
  ApiError,
  cancelJob,
  deleteJobHistory,
  deleteWorkflow,
  getJob,
  getPerformanceCapabilities,
  getPerformanceCurrent,
  getPerformanceExport,
  getPerformanceOverview,
  healthCheck,
  listJobs,
  listWorkflows,
  rerunJob,
  runByName,
  saveWorkflow,
  submitJob,
  submitJobWithParams,
} from '../client';

beforeEach(() => {
  vi.stubGlobal('fetch', vi.fn());
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe('request() via healthCheck()', () => {
  it('throws ApiError on non-200 response', async () => {
    const mockFetch = vi.mocked(fetch);
    mockFetch.mockResolvedValueOnce(
      new Response(JSON.stringify({ error: 'Not Found' }), {
        status: 404,
        statusText: 'Not Found',
        headers: { 'Content-Type': 'application/json' },
      }),
    );

    await expect(healthCheck()).rejects.toThrow(ApiError);
    await expect(
      healthCheck().catch((e: unknown) => {
        if (e instanceof ApiError) {
          expect(e.status).toBe(404);
          expect(e.message).toBe('Not Found');
          expect(e.name).toBe('ApiError');
        }
        throw e;
      }),
    ).rejects.toThrow();
  });

  it('returns parsed JSON on success', async () => {
    const mockFetch = vi.mocked(fetch);
    mockFetch.mockResolvedValueOnce(
      new Response(JSON.stringify({ status: 'ok' }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      }),
    );

    const result = await healthCheck();
    expect(result).toEqual({ status: 'ok' });
    expect(mockFetch).toHaveBeenCalledWith('/api/health', undefined);
  });
});

describe('ApiError', () => {
  it('has correct properties', () => {
    const err = new ApiError(500, 'Internal Server Error', { detail: 'oops' });

    expect(err.name).toBe('ApiError');
    expect(err.status).toBe(500);
    expect(err.message).toBe('Internal Server Error');
    expect(err.body).toEqual({ detail: 'oops' });
    expect(err).toBeInstanceOf(Error);
    expect(err).toBeInstanceOf(ApiError);
  });
});

describe('performance API validators', () => {
  it.each([
    {
      status: 'disabled' as const,
      enabled: false,
      reason: 'disabled_by_config',
      message: 'telemetry disabled',
      metrics: null,
    },
    {
      status: 'degraded' as const,
      enabled: true,
      reason: 'sampler_stale',
      message: 'degraded telemetry',
      metrics: {
        cpu_util_percent: 17.4,
        gpu_util_percent: null,
      },
    },
    {
      status: 'partial' as const,
      enabled: true,
      reason: 'gpu_missing',
      message: 'partial telemetry',
      metrics: {
        cpu_util_percent: 19.8,
      },
    },
  ])(
    'validates /api/performance/current envelope for $status with enabled=$enabled',
    async ({ status, enabled, reason, message, metrics }) => {
      vi.mocked(fetch).mockResolvedValueOnce(
        new Response(
          JSON.stringify({
            status,
            enabled,
            reason,
            message,
            metrics,
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        ),
      );

      const result = await getPerformanceCurrent();
      expect(result.status).toBe(status);
      expect(result.enabled).toBe(enabled);
      expect(result.reason).toBe(reason);
      expect(fetch).toHaveBeenCalledWith('/api/performance/current', undefined);
    },
  );

  it('validates /api/performance/current payload shape', async () => {
    vi.mocked(fetch).mockResolvedValueOnce(
      new Response(
        JSON.stringify({
          status: 'enabled',
          enabled: true,
          reason: 'collector_ok',
          message: 'telemetry available',
          metrics: {
            cpu_util_percent: 31.2,
            gpu_util_percent: null,
            ram_used_bytes: 123456,
          },
        }),
        { status: 200, headers: { 'Content-Type': 'application/json' } },
      ),
    );

    const result = await getPerformanceCurrent();
    expect(result.status).toBe('enabled');
    expect(result.enabled).toBe(true);
    expect(result.metrics?.cpu_util_percent).toBe(31.2);
    expect(fetch).toHaveBeenCalledWith('/api/performance/current', undefined);
  });

  it('throws ApiError when /api/performance/current payload is malformed', async () => {
    vi.mocked(fetch).mockResolvedValueOnce(
      new Response(
        JSON.stringify({
          status: 'enabled',
          enabled: true,
          reason: 'collector_ok',
          message: 'telemetry available',
          metrics: {
            cpu_util_percent: '31.2',
          },
        }),
        { status: 200, headers: { 'Content-Type': 'application/json' } },
      ),
    );

    await expect(getPerformanceCurrent()).rejects.toMatchObject({
      name: 'ApiError',
      status: 500,
      message: 'Unexpected response payload from /api/performance/current',
    });
  });

  it('validates /api/performance/overview, /export and /capabilities payloads', async () => {
    vi.mocked(fetch)
      .mockResolvedValueOnce(
        new Response(
          JSON.stringify({
            status: 'partial',
            enabled: true,
            reason: 'gpu_fallback',
            message: 'gpu telemetry is partial',
            metrics: {
              cpu_util_percent: 22,
              gpu_util_percent: null,
            },
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        ),
      )
      .mockResolvedValueOnce(
        new Response(
          JSON.stringify({
            status: 'enabled',
            enabled: true,
            reason: 'collector_ok',
            message: 'telemetry export ready',
            series: [
              {
                timestamp_ms: 1700000000000,
                metrics: {
                  cpu_util_percent: 40,
                  ram_used_bytes: 100,
                },
              },
              {
                timestamp_ms: 1700000001000,
                metrics: {
                  cpu_util_percent: null,
                  ram_used_bytes: 120,
                },
              },
            ],
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        ),
      )
      .mockResolvedValueOnce(
        new Response(
          JSON.stringify({
            status: 'enabled',
            enabled: true,
            reason: 'configured',
            message: 'ready',
            supported_statuses: ['disabled', 'enabled', 'degraded', 'partial'],
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        ),
      );

    const overview = await getPerformanceOverview();
    const exported = await getPerformanceExport();
    const capabilities = await getPerformanceCapabilities();

    expect(overview.status).toBe('partial');
    expect(exported.series).toHaveLength(2);
    expect(exported.series[0].timestamp_ms).toBe(1700000000000);
    expect(capabilities.supported_statuses).toEqual([
      'disabled',
      'enabled',
      'degraded',
      'partial',
    ]);
    expect(fetch).toHaveBeenNthCalledWith(1, '/api/performance/overview', undefined);
    expect(fetch).toHaveBeenNthCalledWith(2, '/api/performance/export', undefined);
    expect(fetch).toHaveBeenNthCalledWith(3, '/api/performance/capabilities', undefined);
  });

  it('validates disabled envelopes keep enabled=false across overview/export/capabilities', async () => {
    vi.mocked(fetch)
      .mockResolvedValueOnce(
        new Response(
          JSON.stringify({
            status: 'disabled',
            enabled: false,
            reason: 'disabled_by_config',
            message: 'telemetry disabled',
            metrics: null,
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        ),
      )
      .mockResolvedValueOnce(
        new Response(
          JSON.stringify({
            status: 'disabled',
            enabled: false,
            reason: 'disabled_by_config',
            message: 'telemetry disabled',
            series: [],
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        ),
      )
      .mockResolvedValueOnce(
        new Response(
          JSON.stringify({
            status: 'disabled',
            enabled: false,
            reason: 'disabled_by_config',
            message: 'telemetry disabled',
            supported_statuses: ['disabled', 'enabled', 'degraded', 'partial'],
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        ),
      );

    const overview = await getPerformanceOverview();
    const exported = await getPerformanceExport();
    const capabilities = await getPerformanceCapabilities();

    expect(overview.status).toBe('disabled');
    expect(overview.enabled).toBe(false);
    expect(overview.metrics).toBeNull();
    expect(exported.status).toBe('disabled');
    expect(exported.enabled).toBe(false);
    expect(exported.series).toEqual([]);
    expect(capabilities.status).toBe('disabled');
    expect(capabilities.enabled).toBe(false);
  });
});

const emptyWorkflow: Workflow = { nodes: [], connections: [] };

describe('listWorkflows()', () => {
  it('calls GET /api/workflows and returns entries', async () => {
    vi.mocked(fetch).mockResolvedValueOnce(
      new Response(
        JSON.stringify([
          {
            filename: 'test.json',
            name: 'Test',
            description: '',
            workflow: emptyWorkflow,
            has_interface: false,
          },
        ]),
        { status: 200, headers: { 'Content-Type': 'application/json' } },
      ),
    );

    const result = await listWorkflows();
    expect(result).toHaveLength(1);
    expect(result[0].filename).toBe('test.json');
    expect(fetch).toHaveBeenCalledWith('/api/workflows', undefined);
  });
});

describe('saveWorkflow()', () => {
  it('calls POST /api/workflows with name, description, workflow', async () => {
    vi.mocked(fetch).mockResolvedValueOnce(
      new Response(
        JSON.stringify({
          filename: 'test.json',
          name: 'Test',
          description: '',
          workflow: emptyWorkflow,
          has_interface: false,
        }),
        { status: 200, headers: { 'Content-Type': 'application/json' } },
      ),
    );

    const result = await saveWorkflow('Test', '', emptyWorkflow);
    expect(result.filename).toBe('test.json');
    expect(fetch).toHaveBeenCalledWith(
      '/api/workflows',
      expect.objectContaining({
        method: 'POST',
        body: expect.stringContaining('"name":"Test"'),
      }),
    );
  });
});

describe('deleteWorkflow()', () => {
  it('calls DELETE /api/workflows/{filename}', async () => {
    vi.mocked(fetch).mockResolvedValueOnce(new Response(null, { status: 200 }));
    await deleteWorkflow('test.json');
    expect(fetch).toHaveBeenCalledWith(
      '/api/workflows/test.json',
      expect.objectContaining({ method: 'DELETE' }),
    );
  });

  it('encodes filename in URL', async () => {
    vi.mocked(fetch).mockResolvedValueOnce(new Response(null, { status: 200 }));
    await deleteWorkflow('my workflow.json');
    expect(fetch).toHaveBeenCalledWith(
      '/api/workflows/my%20workflow.json',
      expect.objectContaining({ method: 'DELETE' }),
    );
  });

  it('throws ApiError on non-200 response', async () => {
    vi.mocked(fetch).mockResolvedValueOnce(
      new Response('Not Found', { status: 404, statusText: 'Not Found' }),
    );
    await expect(deleteWorkflow('nonexistent.json')).rejects.toThrow(ApiError);
  });
});

describe('submitJob()', () => {
  it('calls POST /api/jobs with workflow', async () => {
    vi.mocked(fetch).mockResolvedValueOnce(
      new Response(
        JSON.stringify({
          id: 'j1',
          status: 'queued',
          created_at: '2025-01-01T00:00:00Z',
        }),
        { status: 201, headers: { 'Content-Type': 'application/json' } },
      ),
    );

    const result = await submitJob(emptyWorkflow);
    expect(result.id).toBe('j1');
    expect(fetch).toHaveBeenCalledWith(
      '/api/jobs',
      expect.objectContaining({ method: 'POST' }),
    );
  });
});

describe('submitJobWithParams()', () => {
  it('calls POST /api/jobs with workflow and params', async () => {
    vi.mocked(fetch).mockResolvedValueOnce(
      new Response(
        JSON.stringify({
          id: 'j2',
          status: 'queued',
          created_at: '2025-01-01T00:00:00Z',
        }),
        { status: 201, headers: { 'Content-Type': 'application/json' } },
      ),
    );

    const result = await submitJobWithParams(emptyWorkflow, { greeting: 'hello' });
    expect(result.id).toBe('j2');

    const callBody = vi.mocked(fetch).mock.calls[0][1];
    expect(JSON.parse(callBody?.body as string)).toEqual({
      workflow: emptyWorkflow,
      params: { greeting: 'hello' },
    });
  });
});

describe('runByName()', () => {
  it('calls POST /api/run with workflow_name', async () => {
    vi.mocked(fetch).mockResolvedValueOnce(
      new Response(
        JSON.stringify({
          id: 'j-run',
          status: 'queued',
          created_at: '2025-01-01T00:00:00Z',
        }),
        { status: 201, headers: { 'Content-Type': 'application/json' } },
      ),
    );

    const result = await runByName('named-run');
    expect(result.id).toBe('j-run');

    const callBody = vi.mocked(fetch).mock.calls[0][1];
    expect(JSON.parse(callBody?.body as string)).toEqual({
      workflow_name: 'named-run',
    });
    expect(fetch).toHaveBeenCalledWith(
      '/api/run',
      expect.objectContaining({ method: 'POST' }),
    );
  });

  it('includes params when provided', async () => {
    vi.mocked(fetch).mockResolvedValueOnce(
      new Response(
        JSON.stringify({
          id: 'j-run-params',
          status: 'queued',
          created_at: '2025-01-01T00:00:00Z',
        }),
        { status: 201, headers: { 'Content-Type': 'application/json' } },
      ),
    );

    await runByName('named-run', { quality: 'high', retries: 2 });

    const callBody = vi.mocked(fetch).mock.calls[0][1];
    expect(JSON.parse(callBody?.body as string)).toEqual({
      workflow_name: 'named-run',
      params: { quality: 'high', retries: 2 },
    });
  });
});

describe('listJobs()', () => {
  it('calls GET /api/jobs and returns array', async () => {
    vi.mocked(fetch).mockResolvedValueOnce(
      new Response(JSON.stringify([]), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      }),
    );

    const result = await listJobs();
    expect(result).toEqual([]);
    expect(fetch).toHaveBeenCalledWith('/api/jobs', undefined);
  });
});

describe('getJob()', () => {
  it('calls GET /api/jobs/{id}', async () => {
    vi.mocked(fetch).mockResolvedValueOnce(
      new Response(
        JSON.stringify({
          id: 'j1',
          status: 'running',
          progress: null,
          created_at: '',
          started_at: '',
          completed_at: null,
          error: null,
          workflow_name: 'Manual Workflow',
          workflow_source: 'api_jobs',
          params: null,
          rerun_of_job_id: null,
          duration_ms: null,
        }),
        { status: 200, headers: { 'Content-Type': 'application/json' } },
      ),
    );

    const result = await getJob('j1');
    expect(result.id).toBe('j1');
    expect(fetch).toHaveBeenCalledWith('/api/jobs/j1', undefined);
  });
});

describe('rerunJob()', () => {
  it('calls POST /api/jobs/{id}/rerun', async () => {
    vi.mocked(fetch).mockResolvedValueOnce(
      new Response(
        JSON.stringify({
          id: 'j-rerun',
          status: 'queued',
          created_at: '2025-01-01T00:00:00Z',
        }),
        { status: 201, headers: { 'Content-Type': 'application/json' } },
      ),
    );

    const result = await rerunJob('j1');
    expect(result.id).toBe('j-rerun');
    expect(fetch).toHaveBeenCalledWith(
      '/api/jobs/j1/rerun',
      expect.objectContaining({ method: 'POST' }),
    );
  });
});

describe('deleteJobHistory()', () => {
  it('calls DELETE /api/jobs/{id} and accepts 204', async () => {
    vi.mocked(fetch).mockResolvedValueOnce(new Response(null, { status: 204 }));
    await expect(deleteJobHistory('j1')).resolves.toBeUndefined();
    expect(fetch).toHaveBeenCalledWith('/api/jobs/j1', { method: 'DELETE' });
  });
});

describe('cancelJob()', () => {
  it('remains backward-compatible and delegates to DELETE /api/jobs/{id}', async () => {
    vi.mocked(fetch).mockResolvedValueOnce(new Response(null, { status: 204 }));

    await expect(cancelJob('j1')).resolves.toBeUndefined();
    expect(fetch).toHaveBeenCalledWith(
      '/api/jobs/j1',
      { method: 'DELETE' },
    );
  });
});
