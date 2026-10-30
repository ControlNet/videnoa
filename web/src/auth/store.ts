import { create } from 'zustand';
import { authenticatedFetch, setCsrfToken } from './transport';

export interface Session {
  password_enabled: boolean;
  authenticated: boolean;
  csrf_token?: string | null;
  expires_at?: number | null;
  idle_expires_at?: number | null;
}
interface AuthState {
  session: Session | null;
  error: string | null;
  refresh: () => Promise<void>;
  accept: (session: Session) => void;
}
export const useAuthStore = create<AuthState>((set) => ({
  session: null,
  error: null,
  accept: (session) => { setCsrfToken(session.csrf_token ?? null); set({ session, error: null }); },
  refresh: async () => {
    try {
      const session = await authRequest('/api/auth/session');
      setCsrfToken(session.csrf_token ?? null);
      set({ session, error: null });
    } catch (error) {
      set({ session: null, error: error instanceof Error ? error.message : 'Unable to check access status' });
    }
  },
}));
/** Channel shared by this browser's tabs of this worker UI. */
const AUTH_CHANNEL = 'videnoa-auth';
const PASSWORD_CHANGED = 'password-changed';

// One channel per tab: a channel does not receive its own messages, so the tab
// that changed the password is not told about it again.
let authChannel: BroadcastChannel | null = null;
const passwordChangeListeners = new Set<() => void>();
function channel(): BroadcastChannel | null {
  if (typeof BroadcastChannel === 'undefined') return null;
  if (!authChannel) {
    authChannel = new BroadcastChannel(AUTH_CHANNEL);
    authChannel.onmessage = (event: MessageEvent) => {
      if (event.data === PASSWORD_CHANGED) passwordChangeListeners.forEach((listener) => { listener(); });
    };
  }
  return authChannel;
}

/** Tell this browser's other tabs that the password changed, so they refresh their session. */
export function announcePasswordChange(): void {
  channel()?.postMessage(PASSWORD_CHANGED);
}

/** Call `onChange` when another tab announces a password change; returns the unsubscribe. */
export function onPasswordChangeElsewhere(onChange: () => void): () => void {
  channel();
  passwordChangeListeners.add(onChange);
  return () => { passwordChangeListeners.delete(onChange); };
}

export async function authRequest(url: string, method = 'GET', body?: unknown): Promise<Session> {
  const response = await authenticatedFetch(url, {
    method,
    ...(body === undefined ? {} : { headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) }),
  });
  if (!response.ok) {
    if (response.status === 401) throw new Error('unauthorized');
    if (response.status === 429) throw new Error('rateLimited');
    throw new Error('requestFailed');
  }
  const result: unknown = await response.json();
  if (url.endsWith('/logout')) return { password_enabled: true, authenticated: false };
  if (!result || typeof result !== 'object' || !('password_enabled' in result) || typeof result.password_enabled !== 'boolean' || !('authenticated' in result) || typeof result.authenticated !== 'boolean') throw new Error('requestFailed');
  return result as Session;
}
