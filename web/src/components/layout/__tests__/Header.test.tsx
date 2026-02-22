import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { healthCheck, listJobs } from '@/api/client'
import { i18n, initializeI18n } from '@/i18n'
import { LOCALE_STORAGE_KEY, SUPPORTED_LOCALES } from '@/i18n/locales/types'
import { Header } from '../Header'

vi.mock('@/api/client', () => ({
  healthCheck: vi.fn(),
  listJobs: vi.fn(),
}))

const originalTauri = (globalThis as { __TAURI__?: unknown }).__TAURI__

beforeEach(async () => {
  vi.clearAllMocks()
  initializeI18n()
  await i18n.changeLanguage('en')
  window.localStorage.removeItem(LOCALE_STORAGE_KEY)
  vi.mocked(healthCheck).mockResolvedValue({ status: 'ok' })
  vi.mocked(listJobs).mockResolvedValue([])
  delete (globalThis as { __TAURI__?: unknown }).__TAURI__
})

afterEach(() => {
  if (originalTauri === undefined) {
    delete (globalThis as { __TAURI__?: unknown }).__TAURI__
    return
  }

  ;(globalThis as { __TAURI__?: unknown }).__TAURI__ = originalTauri
})

describe('Header desktop runtime gating', () => {
  it('renders safely in browser runtime', async () => {
    render(
			<MemoryRouter>
				<Header />
			</MemoryRouter>,
		)

		const projectLink = screen.getByRole('link', { name: 'Videnoa' })
		expect(projectLink).toHaveAttribute(
			'href',
			'https://github.com/ControlNet/Videona',
		)
		expect(projectLink).toHaveAttribute('target', '_blank')
		expect(projectLink).toHaveAttribute('rel', 'noopener noreferrer')

		expect(screen.getByRole('banner')).toHaveAttribute('data-desktop-runtime', 'false')
		expect(await screen.findByText('Editor')).toBeInTheDocument()
		expect(screen.queryByRole('button', { name: 'Minimize window' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Toggle maximize window' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Close window' })).not.toBeInTheDocument()
  })

  it('shows desktop controls and dispatches window actions when Tauri window API is present', async () => {
    const minimize = vi.fn()
    const toggleMaximize = vi.fn()
    const close = vi.fn()

    ;(globalThis as { __TAURI__?: unknown }).__TAURI__ = {
      window: {
        getCurrentWindow: () => ({
          minimize,
          toggleMaximize,
          close,
        }),
      },
    }

    render(
      <MemoryRouter>
        <Header />
      </MemoryRouter>,
    )

    expect(await screen.findByText('Editor')).toBeInTheDocument()
    expect(screen.getByRole('banner')).toHaveAttribute('data-desktop-runtime', 'true')

    fireEvent.click(screen.getByRole('button', { name: 'Minimize window' }))
    fireEvent.click(screen.getByRole('button', { name: 'Toggle maximize window' }))
    fireEvent.click(screen.getByRole('button', { name: 'Close window' }))

    expect(minimize).toHaveBeenCalledTimes(1)
    expect(toggleMaximize).toHaveBeenCalledTimes(1)
    expect(close).toHaveBeenCalledTimes(1)
  })

  it('switches locale and persists locale state', async () => {
    render(
      <MemoryRouter>
        <Header />
      </MemoryRouter>,
    )

    const localeMenuTrigger = screen.getByRole('button', { name: 'Select language' })
    fireEvent.pointerDown(localeMenuTrigger, { button: 0, ctrlKey: false })

    const localeOptions = await screen.findAllByRole('menuitemradio')
    expect(localeOptions).toHaveLength(SUPPORTED_LOCALES.length)
    expect(
      screen.getByRole('menuitemradio', { name: 'English' }),
    ).toHaveAttribute('aria-checked', 'true')

    fireEvent.click(screen.getByRole('menuitemradio', { name: '简体中文' }))

    expect(await screen.findByText('编辑器')).toBeInTheDocument()
    await waitFor(() => {
      expect(window.localStorage.getItem(LOCALE_STORAGE_KEY)).toBe('zh-CN')
      expect(document.documentElement.lang).toBe('zh-CN')
    })
  })
})
