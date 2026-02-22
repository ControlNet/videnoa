import type {
  PerformanceCapabilitiesResponse,
  PerformanceCurrentResponse,
  PerformanceEnvelope,
  PerformanceExportResponse,
  PerformanceMetrics,
  PerformanceOverviewResponse,
  PerformanceSeriesPoint,
  PerformanceStatus,
} from '../types';

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

function isPerformanceStatus(value: unknown): value is PerformanceStatus {
  return value === 'disabled' || value === 'enabled' || value === 'degraded' || value === 'partial';
}

function parsePerformanceEnvelope(value: unknown): PerformanceEnvelope | null {
  if (!isRecord(value)) {
    return null;
  }

  const { status, enabled, reason, message } = value;
  if (
    !isPerformanceStatus(status) ||
    typeof enabled !== 'boolean' ||
    typeof reason !== 'string' ||
    typeof message !== 'string'
  ) {
    return null;
  }

  return { status, enabled, reason, message };
}

function parsePerformanceMetrics(value: unknown): PerformanceMetrics | null {
  if (!isRecord(value)) {
    return null;
  }

  const metrics: PerformanceMetrics = {};
  for (const [key, metricValue] of Object.entries(value)) {
    if (metricValue !== null && typeof metricValue !== 'number') {
      return null;
    }
    metrics[key] = metricValue;
  }

  return metrics;
}

function parsePerformanceSeriesPoint(value: unknown): PerformanceSeriesPoint | null {
  if (!isRecord(value)) {
    return null;
  }

  const { timestamp_ms, metrics } = value;
  if (typeof timestamp_ms !== 'number') {
    return null;
  }

  const parsedMetrics = parsePerformanceMetrics(metrics);
  if (!parsedMetrics) {
    return null;
  }

  return {
    timestamp_ms,
    metrics: parsedMetrics,
  };
}

export function parsePerformanceCurrentResponse(value: unknown): PerformanceCurrentResponse | null {
  const envelope = parsePerformanceEnvelope(value);
  if (!envelope || !isRecord(value)) {
    return null;
  }

  const parsedMetrics = value.metrics === null ? null : parsePerformanceMetrics(value.metrics);
  if (parsedMetrics === null && value.metrics !== null) {
    return null;
  }

  return {
    ...envelope,
    metrics: parsedMetrics,
  };
}

export function parsePerformanceOverviewResponse(
  value: unknown,
): PerformanceOverviewResponse | null {
  const envelope = parsePerformanceEnvelope(value);
  if (!envelope || !isRecord(value)) {
    return null;
  }

  const parsedMetrics = value.metrics === null ? null : parsePerformanceMetrics(value.metrics);
  if (parsedMetrics === null && value.metrics !== null) {
    return null;
  }

  return {
    ...envelope,
    metrics: parsedMetrics,
  };
}

export function parsePerformanceExportResponse(value: unknown): PerformanceExportResponse | null {
  const envelope = parsePerformanceEnvelope(value);
  if (!envelope || !isRecord(value) || !Array.isArray(value.series)) {
    return null;
  }

  const series: PerformanceSeriesPoint[] = [];
  for (const row of value.series) {
    const parsedRow = parsePerformanceSeriesPoint(row);
    if (!parsedRow) {
      return null;
    }
    series.push(parsedRow);
  }

  return {
    ...envelope,
    series,
  };
}

export function parsePerformanceCapabilitiesResponse(
  value: unknown,
): PerformanceCapabilitiesResponse | null {
  const envelope = parsePerformanceEnvelope(value);
  if (!envelope || !isRecord(value) || !Array.isArray(value.supported_statuses)) {
    return null;
  }

  const supportedStatuses: PerformanceStatus[] = [];
  for (const status of value.supported_statuses) {
    if (!isPerformanceStatus(status)) {
      return null;
    }
    supportedStatuses.push(status);
  }

  return {
    ...envelope,
    supported_statuses: supportedStatuses,
  };
}
