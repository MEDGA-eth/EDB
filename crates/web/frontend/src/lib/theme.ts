export type Theme = 'light' | 'dark';

const STORAGE_KEY = 'edb-web:theme';

export function getStoredTheme(): Theme | null {
  if (typeof localStorage === 'undefined') return null;
  const v = localStorage.getItem(STORAGE_KEY);
  return v === 'light' || v === 'dark' ? v : null;
}

export function getSystemTheme(): Theme {
  if (typeof window === 'undefined' || !window.matchMedia) return 'light';
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}

export function applyTheme(theme: Theme): void {
  if (typeof document === 'undefined') return;
  document.documentElement.setAttribute('data-theme', theme);
}

export function persistTheme(theme: Theme): void {
  if (typeof localStorage === 'undefined') return;
  try {
    localStorage.setItem(STORAGE_KEY, theme);
  } catch (e) {
    // QuotaExceededError, SecurityError (private mode / iframes), etc.
    // eslint-disable-next-line no-console
    console.warn('persistTheme: localStorage.setItem failed', e);
  }
}

export function initialTheme(): Theme {
  return getStoredTheme() ?? getSystemTheme();
}
