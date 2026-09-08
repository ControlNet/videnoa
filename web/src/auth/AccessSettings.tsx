import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { CheckRow, Field, SettingsGrid, SettingsSection, settingsInputClass } from '@/pages/settings/section';
import type { AuthConfig } from '@/types';
import { PasswordField } from './PasswordField';
import { authRequest, useAuthStore } from './store';

/**
 * Changing the password and choosing how long a session lives are two different
 * decisions on two different clocks -- one applies at once, the other rides the
 * page's Save -- so they are two sections rather than one card with a rule.
 */
export function PasswordSettings() {
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

  return <SettingsSection id="settings-password" title={t('auth.accessTitle')} note={t('auth.passwordActionHint')}>
    <SettingsGrid>
      <PasswordField id="access-password" label={t('auth.newPassword')} newPassword dense value={password} onChange={setPassword} className="sm:col-span-2" />
      <PasswordField id="access-confirmation" label={t('auth.confirmPassword')} newPassword dense value={confirmation} onChange={setConfirmation} className="sm:col-span-2" />
      <div className="flex items-end gap-1.5 sm:col-span-2 sm:self-end">
        <Button size="sm" className="h-8" disabled={busy || !password || !confirmation} onClick={() => void changePassword(false)}>{t(enabled ? 'auth.changePassword' : 'auth.enablePassword')}</Button>
        {enabled && <Button size="sm" variant="outline" className="h-8" disabled={busy} onClick={() => { setConfirmDisable(true); }}>{t('auth.disablePassword')}</Button>}
      </div>
    </SettingsGrid>
    {error && <p role="alert" className="text-[11px] font-semibold text-destructive">{t(`auth.${error}`)}</p>}
    {success && <p role="status" className="text-[11px] text-muted-foreground">{t('auth.saved')}</p>}
    <Dialog open={confirmDisable} onOpenChange={setConfirmDisable}><DialogContent><DialogHeader><DialogTitle>{t('auth.disablePassword')}</DialogTitle><DialogDescription>{t('auth.disableConfirm')}</DialogDescription></DialogHeader><DialogFooter><Button variant="outline" disabled={busy} onClick={() => { setConfirmDisable(false); }}>{t('auth.cancel')}</Button><Button variant="destructive" disabled={busy} onClick={() => void changePassword(true)}>{t('auth.disablePassword')}</Button></DialogFooter></DialogContent></Dialog>
  </SettingsSection>;
}

export function SessionSettings({ config, onChange }: { config: AuthConfig; onChange: (next: AuthConfig) => void }) {
  const { t } = useTranslation('common');
  return <SettingsSection id="settings-session" title={t('auth.sessionTitle')} note={t('auth.lifetimeHint')}>
    <SettingsGrid>
      <Field id="access-absolute-seconds" label={t('auth.absoluteSeconds')} hint={t('auth.secondsUnit')}>
        <Input id="access-absolute-seconds" className={settingsInputClass} type="number" min={1} step={1} value={config.session_absolute_seconds} onChange={(event) => { onChange({ ...config, session_absolute_seconds: Number(event.target.value) }); }} />
      </Field>
      <Field id="access-idle-seconds" label={t('auth.idleSeconds')} hint={t('auth.secondsUnit')}>
        <Input id="access-idle-seconds" className={settingsInputClass} type="number" min={1} step={1} value={config.session_idle_seconds} onChange={(event) => { onChange({ ...config, session_idle_seconds: Number(event.target.value) }); }} />
      </Field>
      <CheckRow id="access-secure-cookie" className="sm:col-span-3 sm:self-end" label={t('auth.secureCookie')} checked={config.secure_cookie} onChange={(event) => { onChange({ ...config, secure_cookie: event.target.checked }); }} />
    </SettingsGrid>
    <p className="text-[11px] leading-4 text-muted-foreground">{t('auth.secureHint')}</p>
  </SettingsSection>;
}
