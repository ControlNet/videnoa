// Browser credentials remain in an HttpOnly cookie; only CSRF state lives here.
let csrfToken: string | null = null;
export function setCsrfToken(token: string | null): void { csrfToken = token; }

const SAFE_METHODS = ['GET', 'HEAD', 'OPTIONS'];
const AUTH_URLS = ['/api/auth/login', '/api/auth/session'];

let tokenRefresh: Promise<string | null> | null = null;

/*
 * A password change (in this browser's other tab or elsewhere) rotates the
 * session, so the CSRF token this tab holds goes stale and its next write is
 * rejected with 403. Re-read the session for a fresh token -- one read shared
 * by every write rejected while it is in flight -- so the caller can retry
 * exactly once.
 */
function refreshCsrfToken(): Promise<string | null> {
  tokenRefresh ??= (async () => {
    try {
      const response = await fetch('/api/auth/session');
      if (!response.ok) return null;
      const session: unknown = await response.json();
      if (!session || typeof session !== 'object') return null;
      const token = 'csrf_token' in session && typeof session.csrf_token === 'string' ? session.csrf_token : null;
      setCsrfToken(token);
      if (token === null && 'authenticated' in session && session.authenticated === false && 'password_enabled' in session && session.password_enabled === true) {
        window.dispatchEvent(new Event('videnoa-auth-expired'));
      }
      return token;
    } catch {
      return null;
    } finally {
      tokenRefresh = null;
    }
  })();
  return tokenRefresh;
}

function send(url: string, init: RequestInit | undefined, method: string): Promise<Response> {
  if (!csrfToken || SAFE_METHODS.includes(method)) return fetch(url, init);
  const headers = new Headers(init?.headers);
  headers.set('x-csrf-token', csrfToken);
  return fetch(url, { ...init, headers });
}

export async function authenticatedFetch(url: string, init?: RequestInit): Promise<Response> {
  const method = (init?.method ?? 'GET').toUpperCase();
  const sentToken = csrfToken;
  let response = await send(url, init, method);
  if (response.status === 403 && sentToken && !SAFE_METHODS.includes(method) && !url.startsWith('/api/auth/')) {
    const token = await refreshCsrfToken();
    if (token !== null) response = await send(url, init, method);
  }
  if (response.status === 401 && !AUTH_URLS.includes(url)) {
    window.dispatchEvent(new Event('videnoa-auth-expired'));
  }
  return response;
}
