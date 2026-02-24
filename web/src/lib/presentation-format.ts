import { i18n } from '@/i18n';
import { FALLBACK_LOCALE } from '@/i18n/locales/types';

const EM_DASH = '—';

function resolveLocale(locale?: string): string {
  if (locale && locale.trim().length > 0) return locale;
  return i18n.resolvedLanguage || i18n.language || FALLBACK_LOCALE;
}

function isZhLocale(locale: string): boolean {
  return locale.toLowerCase().startsWith('zh');
}

function toTimestamp(value: string | number | Date): number {
  if (value instanceof Date) return value.getTime();
  return new Date(value).getTime();
}

function formatHhMmSs(totalSeconds: number): string {
  const h = Math.floor(totalSeconds / 3600);
  const m = Math.floor((totalSeconds % 3600) / 60);
  const s = totalSeconds % 60;
  return `${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
}

export function formatDurationRange(startedAt: string, completedAt: string | null): string {
  const start = toTimestamp(startedAt);
  if (!Number.isFinite(start)) return EM_DASH;

  const end = completedAt ? toTimestamp(completedAt) : Date.now();
  if (!Number.isFinite(end)) return EM_DASH;

  const totalSeconds = Math.floor((end - start) / 1000);
  if (totalSeconds < 0) return EM_DASH;

  return formatHhMmSs(totalSeconds);
}

export function formatEta(seconds: number | null, locale?: string): string {
  if (seconds == null || !Number.isFinite(seconds) || seconds <= 0) return EM_DASH;

  const resolvedLocale = resolveLocale(locale);
  const remainingLabel = isZhLocale(resolvedLocale) ? '剩余' : 'remaining';
  const totalSeconds = Math.floor(seconds);
  return `~${formatHhMmSs(totalSeconds)} ${remainingLabel}`;
}

export function formatRelativeTime(
  dateInput: string | number | Date,
  options?: { locale?: string; now?: number },
): string {
  const locale = resolveLocale(options?.locale);
  const timestamp = toTimestamp(dateInput);
  if (!Number.isFinite(timestamp)) return EM_DASH;

  const now = options?.now ?? Date.now();
  const diff = Math.floor((now - timestamp) / 1000);

  if (diff < 0) return isZhLocale(locale) ? '刚刚' : 'just now';
  if (diff < 60) return isZhLocale(locale) ? `${String(diff)}秒前` : `${String(diff)}s ago`;

  const minutes = Math.floor(diff / 60);
  if (minutes < 60) {
    return isZhLocale(locale) ? `${String(minutes)}分钟前` : `${String(minutes)} min ago`;
  }

  const hours = Math.floor(minutes / 60);
  if (hours < 24) {
    return isZhLocale(locale)
      ? `${String(hours)}小时前`
      : `${String(hours)} hour${hours !== 1 ? 's' : ''} ago`;
  }

  const days = Math.floor(hours / 24);
  return isZhLocale(locale) ? `${String(days)}天前` : `${String(days)} day${days !== 1 ? 's' : ''} ago`;
}

export function formatCompactNumber(value: number, locale?: string): string {
  if (!Number.isFinite(value)) return String(value);

  const resolvedLocale = resolveLocale(locale);
  if (!isZhLocale(resolvedLocale)) {
    const abs = Math.abs(value);
    if (abs >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
    if (abs >= 1_000) return `${(value / 1_000).toFixed(1)}K`;
    return String(value);
  }

  return new Intl.NumberFormat(resolvedLocale, {
    notation: 'compact',
    compactDisplay: 'short',
    maximumFractionDigits: 1,
  }).format(value);
}

export function formatLocaleDateTime(
  dateInput: string | number | Date,
  options?: Intl.DateTimeFormatOptions,
  locale?: string,
): string {
  const timestamp = toTimestamp(dateInput);
  if (!Number.isFinite(timestamp)) return EM_DASH;

  return new Date(timestamp).toLocaleString(resolveLocale(locale), options);
}
