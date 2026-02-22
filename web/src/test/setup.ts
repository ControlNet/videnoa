import '@testing-library/jest-dom/vitest';
import { beforeEach } from 'vitest';

import { i18n, initializeI18n } from '@/i18n';
import { FALLBACK_LOCALE, LOCALE_STORAGE_KEY } from '@/i18n/locales/types';

if (!window.matchMedia) {
  Object.defineProperty(window, 'matchMedia', {
    writable: true,
    value: (query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addListener: () => {},
      removeListener: () => {},
      addEventListener: () => {},
      removeEventListener: () => {},
      dispatchEvent: () => false,
    }),
  });
}

if (!HTMLElement.prototype.scrollIntoView) {
  HTMLElement.prototype.scrollIntoView = () => {};
}

beforeEach(async () => {
  initializeI18n();
  window.localStorage.setItem(LOCALE_STORAGE_KEY, FALLBACK_LOCALE);
  await i18n.changeLanguage(FALLBACK_LOCALE);
});
