import { Copy } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useAuthStore } from '@/auth/store';
import { authenticatedFetch } from '@/auth/transport';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { CheckRow, Field, SettingsGrid, SettingsSection, settingsInputClass } from './section';

interface Status { enabled: boolean; running: boolean; endpoint_id: string | null; error: string | null }

export function IrohSettings({ enabled, savedEnabled, onChange }: { enabled: boolean; savedEnabled: boolean; onChange: (enabled: boolean) => void }) {
  const { t } = useTranslation('settings');
  const passwordEnabled = useAuthStore((state) => state.session?.password_enabled ?? false);
  const [status, setStatus] = useState<Status | null>(null);
  const [error, setError] = useState(false);
  const [copied, setCopied] = useState(false);
  const [copyFailed, setCopyFailed] = useState(false);
  const idInput = useRef<HTMLInputElement>(null);
  async function copyId() {
    if (!status?.endpoint_id) return;
    try {
      if (navigator.clipboard?.writeText) await navigator.clipboard.writeText(status.endpoint_id);
      else {
        idInput.current?.focus();
        idInput.current?.select();
        if (!document.execCommand('copy')) throw new Error('Manual copy required');
      }
      setCopied(true); setCopyFailed(false);
    } catch { setCopied(false); setCopyFailed(true); idInput.current?.focus(); idInput.current?.select(); }
  }
  useEffect(() => {
    let active = true;
    async function refresh() {
      try {
        const response = await authenticatedFetch('/api/iroh');
        if (!response.ok) throw new Error('Unable to read iroh status');
        const value: unknown = await response.json();
        if (!value || typeof value !== 'object' || !('running' in value) || typeof value.running !== 'boolean' || !('endpoint_id' in value) || (value.endpoint_id !== null && typeof value.endpoint_id !== 'string') || !('enabled' in value) || typeof value.enabled !== 'boolean' || !('error' in value) || (value.error !== null && typeof value.error !== 'string')) throw new Error('Invalid iroh status');
        if (active) { setStatus(value as Status); setError(false); }
      } catch { if (active) setError(true); }
    }
    void refresh();
    const timer = window.setInterval(() => void refresh(), 5000);
    return () => { active = false; window.clearInterval(timer); };
  }, [savedEnabled, passwordEnabled]);

  const failed = error || status?.error !== null && status?.error !== undefined;
  const stateKey = failed ? 'iroh.error' : status?.running ? 'iroh.running' : 'iroh.stopped';

  return <SettingsSection
    id="settings-remote"
    title={t('iroh.title')}
    note={<span role="status" className={failed ? 'text-destructive' : undefined}>{t(stateKey)}</span>}
  >
    <p className="text-[13px] leading-5 text-muted-foreground">{t(passwordEnabled ? 'iroh.description' : 'iroh.passwordRequired')}</p>
    <SettingsGrid>
      <CheckRow
        id="settings-iroh-enabled"
        className="sm:col-span-2 sm:self-end"
        label={t('iroh.enable')}
        checked={enabled}
        disabled={!passwordEnabled && !enabled}
        onChange={(event) => { onChange(event.target.checked); }}
      />
      <Field id="settings-iroh-endpoint" label="Endpoint ID" className="sm:col-span-4">
        <div className="flex gap-1.5">
          <Input
            ref={idInput}
            id="settings-iroh-endpoint"
            aria-label="Endpoint ID"
            className={`${settingsInputClass} text-muted-foreground`}
            readOnly
            value={status?.endpoint_id ?? ''}
          />
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-8 shrink-0"
            aria-label={t(copied ? 'iroh.copied' : 'iroh.copy')}
            disabled={!status?.endpoint_id}
            onClick={() => void copyId()}
          >
            <Copy className="size-3.5" />
            {t(copied ? 'iroh.copiedShort' : 'iroh.copyShort')}
          </Button>
        </div>
      </Field>
    </SettingsGrid>
    {copyFailed && <p role="alert" className="text-[11px] text-destructive">{t('iroh.copyFailed')}</p>}
  </SettingsSection>;
}
