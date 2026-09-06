import { useEffect, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { useJobStore } from '@/stores/job-store';
import { useUIStore } from '@/stores/ui-store';
import { resolveStartupLocale } from '@/i18n';
import { PasswordField } from './PasswordField';
import { authRequest, useAuthStore } from './store';
import { setCsrfToken } from './transport';

export function AuthGate({ children }: { children: ReactNode }) {
  const { session, error, refresh, accept } = useAuthStore();
  const { t, i18n } = useTranslation('common');
  const [password, setPassword] = useState('');
  const [busy, setBusy] = useState(false);
  const [loginError, setLoginError] = useState<string | null>(null);
  useEffect(() => { void refresh(); }, [refresh]);
  useEffect(() => {
    const expired = () => {
      setCsrfToken(null);
      useAuthStore.setState({ session: { password_enabled: true, authenticated: false }, error: null });
      useJobStore.getState().unsubscribeFromJob();
      useJobStore.setState({ jobs: [], runtimePreviewsByNodeId: {}, runtimePreviewsByJobId: {} });
      useUIStore.getState().closeModal();
    };
    window.addEventListener('videnoa-auth-expired', expired);
    return () => window.removeEventListener('videnoa-auth-expired', expired);
  }, []);
  const allowed = session !== null && (!session.password_enabled || session.authenticated);
  useEffect(() => {
    if (session === null || allowed) return;
    // A different browser may disable protection while this login screen is open.
    const check = () => { void refresh(); };
    const timer = window.setInterval(check, 10000);
    window.addEventListener('focus', check);
    return () => { window.clearInterval(timer); window.removeEventListener('focus', check); };
  }, [session, allowed, refresh]);
  useEffect(() => {
    if (allowed) void resolveStartupLocale().then((locale) => i18n.changeLanguage(locale));
  }, [allowed, i18n]);
  if (allowed) return children;
  return <main className="min-h-screen flex items-center justify-center bg-background px-5 py-12 text-foreground">
    <Card className="w-full max-w-sm">
      <CardHeader><p className="text-sm text-muted-foreground">videnoa</p><CardTitle><h1>{t('auth.loginTitle')}</h1></CardTitle></CardHeader>
      <CardContent>
        {!session ? <div className="space-y-4"><p role="status">{t(error ? 'auth.requestFailed' : 'auth.checking')}</p>{error && <Button onClick={() => void refresh()}>{t('auth.retry')}</Button>}</div> :
          <form className="space-y-5" onSubmit={(event) => {
            event.preventDefault(); setBusy(true); setLoginError(null);
            void authRequest('/api/auth/login', 'POST', { password }).then((next) => { setPassword(''); accept(next); }).catch((err: unknown) => setLoginError(err instanceof Error ? err.message : 'requestFailed')).finally(() => setBusy(false));
          }}>
            <PasswordField id="login-password" label={t('auth.password')} value={password} onChange={setPassword} />
            {loginError && <p role="alert" className="text-sm text-destructive">{t(`auth.${loginError}`)}</p>}
            <Button className="w-full" type="submit" disabled={busy || !password}>{t(busy ? 'auth.signingIn' : 'auth.signIn')}</Button>
          </form>}
      </CardContent>
    </Card>
  </main>;
}
