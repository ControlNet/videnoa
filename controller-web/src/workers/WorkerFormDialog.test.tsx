import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { expect, it, vi } from 'vitest'
import { workerSchema } from '../api/workerSchemas'
import { WorkerFormDialog } from './WorkerFormDialog'

const worker = workerSchema.parse({
  id: '550e8400-e29b-41d4-a716-446655440000', version: 1, name: 'test worker', api_url: 'http://127.0.0.1:13000/',
  enabled: true, online: false, compute_slots: 1, has_password: true,
  capabilities: { workflows: [], refreshed_at: null },
  capacity: { used_slots: 0, available_slots: 1, assigned_tasks: 0, staged_tasks: 0, processing_tasks: 0, active_uploads: 0, active_downloads: 0, progress: null },
  last_seen_at: null, last_assigned_at: null, created_at: '2030-01-01T00:00:00Z', updated_at: '2030-01-01T00:00:00Z', last_error: null,
})
it('keeps a saved password without exposing it and supports explicit replacement and removal', async () => {
  const onUpdate = vi.fn().mockResolvedValue(false)
  render(<WorkerFormDialog worker={worker} open submitting={false} actionError={null} onClose={vi.fn()} onCreate={vi.fn()} onUpdate={onUpdate} />)
  const input = screen.getByLabelText('Access password (optional)')
  expect(input).toHaveValue('')
  expect(input).toHaveAttribute('type', 'password')
  fireEvent.click(screen.getByRole('button', { name: 'Save Worker' }))
  await waitFor(() => expect(onUpdate).toHaveBeenCalledTimes(1))
  expect(onUpdate.mock.calls[0]?.[1]).not.toHaveProperty('password')
  fireEvent.change(input, { target: { value: 'test-only-password' } })
  fireEvent.click(screen.getByRole('button', { name: 'Save Worker' }))
  await waitFor(() => expect(onUpdate).toHaveBeenCalledTimes(2))
  expect(onUpdate.mock.calls[1]?.[1]).toHaveProperty('password', 'test-only-password')
  fireEvent.click(screen.getByLabelText('Clear saved password'))
  expect(input).toHaveValue('')
  expect(input).toBeDisabled()
  fireEvent.click(screen.getByRole('button', { name: 'Save Worker' }))
  await waitFor(() => expect(onUpdate).toHaveBeenCalledTimes(3))
  expect(onUpdate.mock.calls[2]?.[1]).toHaveProperty('password', null)
})

it('registers an iroh Endpoint ID and password without an HTTP URL', async () => {
  // Synthetic public ID and credential for the form contract only.
  const endpointId = 'a'.repeat(64)
  const onCreate = vi.fn().mockResolvedValue(false)
  render(<WorkerFormDialog worker={null} open submitting={false} actionError={null} onClose={vi.fn()} onCreate={onCreate} onUpdate={vi.fn()} />)
  fireEvent.change(screen.getByLabelText('Worker name'), { target: { value: 'iroh worker' } })
  // Both connection types are on screen, so choosing one is a click, not a menu.
  expect(screen.getByRole('radio', { name: 'HTTP / HTTPS' })).toBeChecked()
  fireEvent.click(screen.getByRole('radio', { name: 'Iroh' }))
  expect(screen.getByRole('radio', { name: 'Iroh' })).toBeChecked()
  fireEvent.change(screen.getByLabelText('Worker Endpoint ID'), { target: { value: endpointId } })
  fireEvent.click(screen.getByRole('button', { name: 'Save Worker' }))
  expect(onCreate).not.toHaveBeenCalled()
  fireEvent.change(screen.getByLabelText(/^Access password/), { target: { value: 'test-only-iroh-password' } })
  fireEvent.click(screen.getByRole('button', { name: 'Save Worker' }))
  await waitFor(() => expect(onCreate).toHaveBeenCalledTimes(1))
  expect(onCreate.mock.calls[0]?.[0]).toMatchObject({ transport: 'iroh', endpoint_id: endpointId, password: 'test-only-iroh-password' })
  expect(onCreate.mock.calls[0]?.[0]).not.toHaveProperty('api_url')
})
