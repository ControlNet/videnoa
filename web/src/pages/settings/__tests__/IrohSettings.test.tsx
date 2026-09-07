import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { useAuthStore } from '@/auth/store';
import { IrohSettings } from '../IrohSettings';
import { i18n } from '@/i18n';

afterEach(() => { cleanup(); vi.unstubAllGlobals(); useAuthStore.setState({ session: null }); });

it('shows no identity while disabled and requires a worker password before enabling', async () => {
  await i18n.changeLanguage('en');
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({ enabled: false, running: false, endpoint_id: null, error: null }))));
  const onChange = vi.fn();
  render(<IrohSettings enabled={false} savedEnabled={false} onChange={onChange} />);
  expect(screen.getByRole('checkbox')).toBeDisabled();
  await waitFor(() => expect(screen.getByRole('status')).toHaveTextContent('Iroh is stopped'));
  expect(screen.getByLabelText('Endpoint ID')).toHaveValue('');
  expect(screen.getByRole('button', { name: 'Copy Endpoint ID' })).toBeDisabled();
  expect(screen.getByLabelText('Endpoint ID')).toHaveAttribute('readonly');
});

it('shows the initialized public identity when enabled', async () => {
  await i18n.changeLanguage('en');
  useAuthStore.setState({ session: { password_enabled: true, authenticated: true } });
  // Test-only public identity and status response; no production endpoint is used.
  const id = 'a'.repeat(64);
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({ enabled: true, running: true, endpoint_id: id, error: null }))));
  render(<IrohSettings enabled={true} savedEnabled={true} onChange={vi.fn()} />);
  await waitFor(() => expect(screen.getByLabelText('Endpoint ID')).toHaveValue(id));
  expect(screen.getByRole('button', { name: 'Copy Endpoint ID' })).toBeEnabled();
});

it('uses the existing settings save flow when enabling', async () => {
  await i18n.changeLanguage('en');
  useAuthStore.setState({ session: { password_enabled: true, authenticated: true } });
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({ enabled: false, running: false, endpoint_id: null, error: null }))));
  const onChange = vi.fn();
  render(<IrohSettings enabled={false} savedEnabled={false} onChange={onChange} />);
  await waitFor(() => expect(fetch).toHaveBeenCalledTimes(1));
  fireEvent.click(screen.getByRole('checkbox'));
  expect(onChange).toHaveBeenCalledWith(true);
});
