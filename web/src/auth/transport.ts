// Browser credentials remain in an HttpOnly cookie; only CSRF state lives here.
let csrfToken: string | null = null;
export function setCsrfToken(token: string | null): void { csrfToken = token; }

export async function authenticatedFetch(url: string, init?: RequestInit): Promise<Response> {
  const headers = new Headers(init?.headers);
  const method = (init?.method ?? 'GET').toUpperCase();
  if (csrfToken && !['GET', 'HEAD', 'OPTIONS'].includes(method)) headers.set('x-csrf-token', csrfToken);
  const response = await fetch(url, csrfToken && !['GET', 'HEAD', 'OPTIONS'].includes(method) ? { ...init, headers } : init);
  if (response.status === 401 && !['/api/auth/login', '/api/auth/session'].includes(url)) {
    window.dispatchEvent(new Event('videnoa-auth-expired'));
  }
  return response;
}
