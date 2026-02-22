import { create } from 'zustand';

type ModalName = 'presets' | 'batch' | 'preview' | 'save' | 'run-params';
export type ThemeMode = 'system' | 'light' | 'dark';

function getSystemTheme(): 'light' | 'dark' {
  if (typeof window === 'undefined') return 'dark';
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}

function getInitialTheme(): ThemeMode {
  if (typeof window === 'undefined') return 'system';
  const stored = localStorage.getItem('theme');
  if (stored === 'light' || stored === 'dark' || stored === 'system') return stored;
  return 'system';
}

function applyThemeClass(mode: ThemeMode) {
  const resolved = mode === 'system' ? getSystemTheme() : mode;
  document.documentElement.classList.toggle('dark', resolved === 'dark');
}

interface UIState {
  sidebarCollapsed: boolean;
  activeModal: ModalName | null;
  theme: ThemeMode;

  toggleSidebar: () => void;
  openModal: (modal: ModalName) => void;
  closeModal: () => void;
  setTheme: (mode: ThemeMode) => void;
}

export const useUIStore = create<UIState>((set) => ({
  sidebarCollapsed: false,
  activeModal: null,
  theme: getInitialTheme(),

  toggleSidebar: () => set((s) => ({ sidebarCollapsed: !s.sidebarCollapsed })),
  openModal: (modal) => set({ activeModal: modal }),
  closeModal: () => set({ activeModal: null }),
  setTheme: (mode) => {
    localStorage.setItem('theme', mode);
    applyThemeClass(mode);
    set({ theme: mode });
  },
}));

// Apply theme on load
applyThemeClass(getInitialTheme());

// Listen for OS theme changes when mode is 'system'
if (typeof window !== 'undefined') {
  window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {
    const { theme } = useUIStore.getState();
    if (theme === 'system') {
      applyThemeClass('system');
    }
  });
}
