import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { AuthGate } from './AuthGate';
import { useAuthStore } from './store';
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
