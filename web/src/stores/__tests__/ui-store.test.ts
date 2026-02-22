import { describe, it, expect, beforeEach, vi } from 'vitest';

Object.defineProperty(window, 'matchMedia', {
  writable: true,
  value: vi.fn().mockImplementation((query: string) => ({
    matches: query === '(prefers-color-scheme: dark)',
    media: query,
    onchange: null,
    addListener: vi.fn(),
    removeListener: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  })),
});

const { useUIStore } = await import('../ui-store');

beforeEach(() => {
  localStorage.clear();
  document.documentElement.classList.remove('dark');
  useUIStore.setState({
    sidebarCollapsed: false,
    activeModal: null,
    theme: 'system',
  });
});

describe('initial state', () => {
  it('has sidebar expanded and no active modal', () => {
    const state = useUIStore.getState();
    expect(state.sidebarCollapsed).toBe(false);
    expect(state.activeModal).toBeNull();
  });
});

describe('toggleSidebar', () => {
  it('flips collapsed to true', () => {
    useUIStore.getState().toggleSidebar();
    expect(useUIStore.getState().sidebarCollapsed).toBe(true);
  });

  it('flips collapsed back to false', () => {
    useUIStore.getState().toggleSidebar();
    useUIStore.getState().toggleSidebar();
    expect(useUIStore.getState().sidebarCollapsed).toBe(false);
  });
});

describe('openModal / closeModal', () => {
  it('sets activeModal to the given name', () => {
    useUIStore.getState().openModal('presets');
    expect(useUIStore.getState().activeModal).toBe('presets');
  });

  it('replaces current modal with a new one', () => {
    useUIStore.getState().openModal('presets');
    useUIStore.getState().openModal('batch');
    expect(useUIStore.getState().activeModal).toBe('batch');
  });

  it('clears activeModal on close', () => {
    useUIStore.getState().openModal('preview');
    useUIStore.getState().closeModal();
    expect(useUIStore.getState().activeModal).toBeNull();
  });

  it('closeModal is safe when no modal is open', () => {
    expect(() => useUIStore.getState().closeModal()).not.toThrow();
    expect(useUIStore.getState().activeModal).toBeNull();
  });
});

describe('setTheme', () => {
  it('sets theme to dark and persists to localStorage', () => {
    useUIStore.getState().setTheme('dark');
    expect(useUIStore.getState().theme).toBe('dark');
    expect(localStorage.getItem('theme')).toBe('dark');
  });

  it('sets theme to light and persists to localStorage', () => {
    useUIStore.getState().setTheme('light');
    expect(useUIStore.getState().theme).toBe('light');
    expect(localStorage.getItem('theme')).toBe('light');
  });

  it('sets theme to system and persists to localStorage', () => {
    useUIStore.getState().setTheme('dark');
    useUIStore.getState().setTheme('system');
    expect(useUIStore.getState().theme).toBe('system');
    expect(localStorage.getItem('theme')).toBe('system');
  });

  it('applies dark class to documentElement when theme is dark', () => {
    useUIStore.getState().setTheme('dark');
    expect(document.documentElement.classList.contains('dark')).toBe(true);
  });

  it('removes dark class when theme is light', () => {
    useUIStore.getState().setTheme('dark');
    useUIStore.getState().setTheme('light');
    expect(document.documentElement.classList.contains('dark')).toBe(false);
  });
});
