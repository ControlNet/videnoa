import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useAuthStore } from '@/auth/store';
import { authenticatedFetch } from '@/auth/transport';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';

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
  return <Card>
    <CardHeader><CardTitle>Iroh</CardTitle></CardHeader>
    <CardContent className="space-y-4">
      <label className="flex items-center gap-2"><input type="checkbox" checked={enabled} disabled={!passwordEnabled && !enabled} onChange={(event) => onChange(event.target.checked)} />{t('iroh.enable')}</label>
      <p className="text-sm text-muted-foreground">{t(passwordEnabled ? 'iroh.description' : 'iroh.passwordRequired')}</p>
      <label className="block space-y-2"><span className="text-sm">Endpoint ID</span><input ref={idInput} aria-label="Endpoint ID" className="w-full rounded border bg-transparent px-3 py-2 font-mono text-sm" readOnly value={status?.endpoint_id ?? ''} /></label>
      <Button variant="outline" disabled={!status?.endpoint_id} onClick={() => void copyId()}>{t(copied ? 'iroh.copied' : 'iroh.copy')}</Button>
      {copyFailed && <p role="alert">{t('iroh.copyFailed')}</p>}
      <p role="status" className="text-sm">{t(error || status?.error ? 'iroh.error' : status?.running ? 'iroh.running' : 'iroh.stopped')}</p>
    </CardContent>
  </Card>;
}
