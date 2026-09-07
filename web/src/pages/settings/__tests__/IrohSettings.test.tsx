import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { useAuthStore } from '@/auth/store';
import { IrohSettings } from '../IrohSettings';
import { i18n } from '@/i18n';

afterEach(() => { cleanup(); vi.unstubAllGlobals(); useAuthStore.setState({ session: null }); });

it('shows the public ID and requires a worker password before enabling', async () => {
  await i18n.changeLanguage('en');
  // Test-only public identity and status response; no production endpoint is used.
  const id = 'a'.repeat(64);
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({ enabled: false, running: false, endpoint_id: id, error: null }))));
  const onChange = vi.fn();
  render(<IrohSettings enabled={false} savedEnabled={false} onChange={onChange} />);
  expect(screen.getByRole('checkbox')).toBeDisabled();
  await waitFor(() => expect(screen.getByLabelText('Endpoint ID')).toHaveValue(id));
  expect(screen.getByLabelText('Endpoint ID')).toHaveAttribute('readonly');
});

it('uses the existing settings save flow when enabling', async () => {
  await i18n.changeLanguage('en');
  useAuthStore.setState({ session: { password_enabled: true, authenticated: true } });
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({ enabled: false, running: false, endpoint_id: null, error: null }))));
  const onChange = vi.fn();
  render(<IrohSettings enabled={false} savedEnabled={false} onChange={onChange} />);
  fireEvent.click(screen.getByRole('checkbox'));
  expect(onChange).toHaveBeenCalledWith(true);
});
