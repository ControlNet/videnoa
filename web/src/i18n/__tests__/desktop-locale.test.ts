import { afterEach, describe, expect, it, vi } from 'vitest';

import { i18n } from '@/i18n';

interface ConfigRequest {
  method: string;
  ifMatch: string | null;
  body: Record<string, unknown> | null;
}

afterEach(() => {
  delete (globalThis as { __TAURI__?: unknown }).__TAURI__;
  vi.unstubAllGlobals();
});

describe('desktop language switch', () => {
  // Settings are one document: the language switch must not revert a change
  // the Settings form saved after the switch read the config.
  it('saves the locale on top of the current settings version', async () => {
    (globalThis as { __TAURI__?: unknown }).__TAURI__ = { window: { appWindow: {} } };
    const versions = [
      { etag: '"v1"', config: { locale: 'en', paths: { models_dir: 'models' } } },
      { etag: '"v2"', config: { locale: 'en', paths: { models_dir: 'saved-by-settings' } } },
    ];
    let current = 0;
    const requests: ConfigRequest[] = [];
    vi.stubGlobal('fetch', vi.fn(async (url: string, init?: RequestInit) => {
      expect(url).toBe('/api/config');
      const method = init?.method ?? 'GET';
      const ifMatch = new Headers(init?.headers).get('If-Match');
      requests.push({ method, ifMatch, body: init?.body ? JSON.parse(String(init.body)) as Record<string, unknown> : null });
      if (method === 'GET') {
        const { etag, config } = versions[current];
        return Response.json(config, { headers: { ETag: etag } });
      }
      if (requests.length === 2) {
        // The Settings form saved between the switch's read and its write.
        current = 1;
        return Response.json({ error: 'changed', code: 'config_changed' }, { status: 412 });
      }
      return ifMatch === versions[current].etag
        ? Response.json(requests.at(-1)?.body, { headers: { ETag: '"v3"' } })
        : Response.json({ error: 'changed', code: 'config_changed' }, { status: 412 });
    }));

    await i18n.changeLanguage('zh-CN');

    await vi.waitFor(() => expect(requests).toHaveLength(4));
    expect(requests.map(({ method, ifMatch }) => [method, ifMatch])).toEqual([
      ['GET', null],
      ['PUT', '"v1"'],
      ['GET', null],
      ['PUT', '"v2"'],
    ]);
    expect(requests[3].body).toEqual({ locale: 'zh-CN', paths: { models_dir: 'saved-by-settings' } });
  });
});
