import { describe, expect, it } from 'vitest';
import { enResources } from '@/i18n/locales/en';
import { I18N_NAMESPACES } from '@/i18n/locales/types';
import { zhCNResources } from '@/i18n/locales/zh-CN';

// A key missing from one locale silently falls back to English (or shows the
// raw key), so both locales must define exactly the same keys.
describe('locale parity', () => {
  for (const namespace of I18N_NAMESPACES) {
    it(`zh-CN and en define the same ${namespace} keys`, () => {
      const en = Object.keys(enResources[namespace]).sort();
      const zh = Object.keys(zhCNResources[namespace]).sort();

      expect({ missingInZhCN: en.filter((key) => !zh.includes(key)) }).toEqual({
        missingInZhCN: [],
      });
      expect({ missingInEn: zh.filter((key) => !en.includes(key)) }).toEqual({
        missingInEn: [],
      });
    });
  }

  it('translates every interpolation placeholder', () => {
    const mismatched: string[] = [];
    for (const namespace of I18N_NAMESPACES) {
      const zh: Record<string, string> = zhCNResources[namespace];
      for (const [key, value] of Object.entries(enResources[namespace])) {
        const placeholders = (text: string) => (text.match(/\{\{\s*\w+\s*\}\}/g) ?? []).sort();
        if (key in zh && placeholders(value).join() !== placeholders(zh[key]).join()) {
          mismatched.push(`${namespace}:${key}`);
        }
      }
    }
    expect(mismatched).toEqual([]);
  });
});
