import { describe, expect, it } from 'vitest';
import {
  formatCompactNumber,
  formatDurationRange,
  formatEta,
  formatLocaleDateTime,
  formatRelativeTime,
} from '../presentation-format';

describe('presentation-format helpers', () => {
  it('formats duration range as hh:mm:ss', () => {
    const start = new Date(0).toISOString();
    const end = new Date(3661 * 1000).toISOString();
    expect(formatDurationRange(start, end)).toBe('01:01:01');
  });

  it('formats ETA while preserving em-dash behavior', () => {
    expect(formatEta(332, 'en')).toBe('~00:05:32 remaining');
    expect(formatEta(null, 'en')).toBe('—');
    expect(formatEta(0, 'en')).toBe('—');
  });

  it('formats relative time in English and Chinese', () => {
    const now = Date.UTC(2025, 0, 1, 0, 0, 0);
    const thirtySecondsAgo = now - 30 * 1000;
    const twoHoursAgo = now - 2 * 60 * 60 * 1000;

    expect(formatRelativeTime(thirtySecondsAgo, { locale: 'en', now })).toBe('30s ago');
    expect(formatRelativeTime(twoHoursAgo, { locale: 'en', now })).toBe('2 hours ago');
    expect(formatRelativeTime(twoHoursAgo, { locale: 'zh-CN', now })).toBe('2小时前');
  });

  it('keeps existing compact-number style in English', () => {
    expect(formatCompactNumber(999, 'en')).toBe('999');
    expect(formatCompactNumber(1_500, 'en')).toBe('1.5K');
    expect(formatCompactNumber(2_000_000, 'en')).toBe('2.0M');
  });

  it('formats locale date-time and handles invalid input safely', () => {
    expect(formatLocaleDateTime('invalid-date', undefined, 'en')).toBe('—');

    const output = formatLocaleDateTime('2025-01-01T00:00:00Z', { timeZone: 'UTC' }, 'en');
    expect(output.length).toBeGreaterThan(0);
  });
});
