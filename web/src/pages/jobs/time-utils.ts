import { formatDurationRange, formatEta } from '@/lib/presentation-format'

export function formatDuration(startedAt: string, completedAt: string | null): string {
  return formatDurationRange(startedAt, completedAt)
}

export function formatETA(seconds: number | null): string {
  return formatEta(seconds)
}
