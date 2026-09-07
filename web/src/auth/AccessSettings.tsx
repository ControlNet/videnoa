import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import type { AuthConfig } from '@/types';
import { PasswordField } from './PasswordField';
import { authRequest, useAuthStore } from './store';

export function AccessSettings({ config, onChange }: { config: AuthConfig; onChange: (next: AuthConfig) => void }) {
  const { t } = useTranslation('common');
  const { session, accept } = useAuthStore();
  const [password, setPassword] = useState('');
  const [confirmation, setConfirmation] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState(false);
  const [confirmDisable, setConfirmDisable] = useState(false);
  const enabled = session?.password_enabled === true;
  async function changePassword(disable: boolean) {
    if (!disable && (password !== confirmation || !password || new TextEncoder().encode(password).length > 1024 || /\p{Cc}/u.test(password))) { setError('passwordInvalid'); return; }
    setBusy(true); setError(null); setSuccess(false);
    try {
      const next = await authRequest('/api/auth/password', disable ? 'DELETE' : 'PUT', disable ? undefined : { password, password_confirmation: confirmation });
      window.dispatchEvent(new Event("videnoa-password-changed"));
      accept(next); setPassword(''); setConfirmation(''); setConfirmDisable(false); setSuccess(true);
    } catch (err) { setError(err instanceof Error ? err.message : 'requestFailed'); }
    finally { setBusy(false); }
  }
  return <Card>
    <CardHeader><CardTitle>{t('auth.accessTitle')}</CardTitle><CardDescription>{t(enabled ? 'auth.enabled' : 'auth.disabled')}</CardDescription></CardHeader>
    <CardContent className="space-y-5">
      <PasswordField id="access-password" label={t('auth.newPassword')} newPassword value={password} onChange={setPassword} />
      <PasswordField id="access-confirmation" label={t('auth.confirmPassword')} newPassword value={confirmation} onChange={setConfirmation} />
      <p className="text-xs text-muted-foreground">{t('auth.passwordActionHint')}</p>
      {error && <p role="alert" className="text-sm text-destructive">{t(`auth.${error}`)}</p>}
      {success && <p role="status" className="text-sm">{t('auth.saved')}</p>}
      <div className="flex flex-wrap gap-2">
        <Button disabled={busy || !password || !confirmation} onClick={() => void changePassword(false)}>{t(enabled ? 'auth.changePassword' : 'auth.enablePassword')}</Button>
        {enabled && <Button variant="outline" disabled={busy} onClick={() => setConfirmDisable(true)}>{t('auth.disablePassword')}</Button>}
        {enabled && <Button variant="ghost" disabled={busy} onClick={() => {
          setBusy(true);
          void authRequest('/api/auth/logout', 'POST').then(() => { window.dispatchEvent(new Event('videnoa-auth-expired')); }).catch(() => setError('requestFailed')).finally(() => setBusy(false));
        }}>{t('auth.signOut')}</Button>}
      </div>
      <div className="border-t pt-5 space-y-4">
        <p className="text-xs text-muted-foreground">{t('auth.lifetimeHint')}</p>
        <label className="block space-y-2"><span className="text-sm">{t('auth.absoluteSeconds')}</span><Input type="number" min={1} step={1} value={config.session_absolute_seconds} onChange={(event) => onChange({ ...config, session_absolute_seconds: Number(event.target.value) })} /></label>
        <label className="block space-y-2"><span className="text-sm">{t('auth.idleSeconds')}</span><Input type="number" min={1} step={1} value={config.session_idle_seconds} onChange={(event) => onChange({ ...config, session_idle_seconds: Number(event.target.value) })} /></label>
        <label className="flex gap-2 items-center text-sm"><input type="checkbox" checked={config.secure_cookie} onChange={(event) => onChange({ ...config, secure_cookie: event.target.checked })} />{t('auth.secureCookie')}</label>
        <p className="text-xs text-muted-foreground">{t('auth.secureHint')}</p>
      </div>
    </CardContent>
    <Dialog open={confirmDisable} onOpenChange={setConfirmDisable}><DialogContent><DialogHeader><DialogTitle>{t('auth.disablePassword')}</DialogTitle><DialogDescription>{t('auth.disableConfirm')}</DialogDescription></DialogHeader><DialogFooter><Button variant="outline" disabled={busy} onClick={() => setConfirmDisable(false)}>{t('auth.cancel')}</Button><Button variant="destructive" disabled={busy} onClick={() => void changePassword(true)}>{t('auth.disablePassword')}</Button></DialogFooter></DialogContent></Dialog>
  </Card>;
}
