export const I18N_NAMESPACES = ['common', 'jobs', 'models', 'settings', 'editor', 'preview'] as const;

export type I18nNamespace = (typeof I18N_NAMESPACES)[number];

export const SUPPORTED_LOCALES = ['en', 'zh-CN'] as const;

export type SupportedLocale = (typeof SUPPORTED_LOCALES)[number];

export const FALLBACK_LOCALE: SupportedLocale = 'en';
export const LOCALE_STORAGE_KEY = 'videnoa.locale';

export type NamespaceResources = Record<I18nNamespace, Record<string, string>>;

export function isSupportedLocale(locale: string): locale is SupportedLocale {
  return SUPPORTED_LOCALES.some((value) => value === locale);
}
