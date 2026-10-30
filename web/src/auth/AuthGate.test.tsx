import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { AuthGate } from './AuthGate';
import { announcePasswordChange, useAuthStore } from './store';
import { authenticatedFetch, setCsrfToken } from './transport';

const testCsrf = crypto.randomUUID();

beforeEach(() => { useAuthStore.setState({ session: null, error: null }); setCsrfToken(null); });
afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

it('does not mount business UI before checking authentication and supports login', async () => {
  const fetcher = vi.fn().mockResolvedValueOnce(Response.json({ password_enabled: true, authenticated: false }))
    .mockResolvedValueOnce(Response.json({ password_enabled: true, authenticated: true, csrf_token: testCsrf }));
  vi.stubGlobal('fetch', fetcher);
  render(<AuthGate><div>Protected editor</div></AuthGate>);
  expect(screen.queryByText('Protected editor')).not.toBeInTheDocument();
  const input = await screen.findByLabelText('Password');
  fireEvent.change(input, { target: { value: 'test-only-password' } });
  fireEvent.click(screen.getByRole('button', { name: 'Sign in' }));
  expect(await screen.findByText('Protected editor')).toBeInTheDocument();
  expect(localStorage.getItem('password')).toBeNull();
  act(() => window.dispatchEvent(new Event('videnoa-auth-expired')));
  expect(screen.queryByText('Protected editor')).not.toBeInTheDocument();
  expect(screen.getByLabelText('Password')).toHaveValue('');
});

it('opens without a password and fails closed if the status endpoint fails', async () => {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(Response.json({ password_enabled: false, authenticated: false })));
  const view = render(<AuthGate><div>Open editor</div></AuthGate>);
  expect(await screen.findByText('Open editor')).toBeInTheDocument();
  view.unmount();
  useAuthStore.setState({ session: null, error: null });
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('', { status: 503 })));
  render(<AuthGate><div>Protected editor</div></AuthGate>);
  expect(await screen.findByRole('button', { name: 'Retry' })).toBeInTheDocument();
  expect(screen.queryByText('Protected editor')).not.toBeInTheDocument();
});

it('adds CSRF to writes and routes business 401 responses to the login boundary', async () => {
  const fetcher = vi.fn().mockResolvedValue(new Response('', { status: 401 }));
  vi.stubGlobal('fetch', fetcher);
  const expired = vi.fn();
  window.addEventListener('videnoa-auth-expired', expired);
  setCsrfToken(testCsrf);
  await authenticatedFetch('/api/jobs/example', { method: 'DELETE' });
  const init = fetcher.mock.calls[0][1] as RequestInit;
  expect(new Headers(init.headers).get('x-csrf-token')).toBe(testCsrf);
  expect(expired).toHaveBeenCalledOnce();
  window.removeEventListener('videnoa-auth-expired', expired);
});

// A password change in another tab rotates the session, so this tab's CSRF
// token goes stale and its next write gets 403.
it('refetches the CSRF token once on a 403 write and retries it once', async () => {
  const fresh = crypto.randomUUID();
  const sessions: string[] = [];
  const fetcher = vi.fn(async (url: string, init?: RequestInit) => {
    if (url === '/api/auth/session') {
      sessions.push(url);
      return Response.json({ password_enabled: true, authenticated: true, csrf_token: fresh });
    }
    const token = new Headers(init?.headers).get('x-csrf-token');
    return new Response('', { status: token === fresh ? 200 : 403 });
  });
  vi.stubGlobal('fetch', fetcher);
  setCsrfToken(testCsrf);

  const [first, second] = await Promise.all([
    authenticatedFetch('/api/workflows', { method: 'POST', body: '{}' }),
    authenticatedFetch('/api/config', { method: 'PUT', body: '{}' }),
  ]);

  expect(first.status).toBe(200);
  expect(second.status).toBe(200);
  expect(sessions).toHaveLength(1);
  expect(fetcher).toHaveBeenCalledTimes(5);
});

it('does not retry reads, auth calls, writes without a token, or a second 403', async () => {
  const fetcher = vi.fn(async (url: string) => url === '/api/auth/session'
    ? Response.json({ password_enabled: true, authenticated: true, csrf_token: crypto.randomUUID() })
    : new Response('', { status: 403 }));
  vi.stubGlobal('fetch', fetcher);

  expect((await authenticatedFetch('/api/workflows', { method: 'POST' })).status).toBe(403);
  expect(fetcher).toHaveBeenCalledTimes(1);

  setCsrfToken(testCsrf);
  fetcher.mockClear();
  expect((await authenticatedFetch('/api/config')).status).toBe(403);
  expect((await authenticatedFetch('/api/auth/logout', { method: 'POST' })).status).toBe(403);
  expect(fetcher).toHaveBeenCalledTimes(2);

  fetcher.mockClear();
  expect((await authenticatedFetch('/api/workflows', { method: 'POST' })).status).toBe(403);
  expect(fetcher.mock.calls.map(([url]) => url)).toEqual(['/api/workflows', '/api/auth/session', '/api/workflows']);
});

it('tells other tabs about a password change so they refresh their session', async () => {
  // Stands in for another tab of the same browser.
  const otherTab = new BroadcastChannel('videnoa-auth');
  const received: unknown[] = [];
  otherTab.onmessage = (event: MessageEvent) => { received.push(event.data); };
  const fetcher = vi.fn().mockResolvedValue(Response.json({ password_enabled: true, authenticated: true, csrf_token: testCsrf }));
  vi.stubGlobal('fetch', fetcher);
  const changed = vi.fn();
  window.addEventListener('videnoa-password-changed', changed);
  render(<AuthGate><div>Protected editor</div></AuthGate>);
  expect(await screen.findByText('Protected editor')).toBeInTheDocument();
  fetcher.mockClear();

  otherTab.postMessage('password-changed');

  await vi.waitFor(() => expect(fetcher).toHaveBeenCalledWith('/api/auth/session', expect.anything()));
  expect(changed).toHaveBeenCalledOnce();
  window.removeEventListener('videnoa-password-changed', changed);

  announcePasswordChange();
  await vi.waitFor(() => expect(received).toEqual(['password-changed']));
  otherTab.close();
});
