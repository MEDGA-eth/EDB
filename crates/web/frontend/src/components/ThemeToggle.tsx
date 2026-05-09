import { useEffect, useState } from 'react';
import { applyTheme, initialTheme, persistTheme, type Theme } from '../lib/theme';

export function ThemeToggle() {
  const [theme, setTheme] = useState<Theme>(() => initialTheme());

  useEffect(() => { applyTheme(theme); }, [theme]);

  function toggle() {
    setTheme(prev => {
      const next: Theme = prev === 'dark' ? 'light' : 'dark';
      persistTheme(next);
      return next;
    });
  }

  return (
    <button
      type="button"
      title={`Switch to ${theme === 'dark' ? 'light' : 'dark'} mode`}
      aria-label={`Switch to ${theme === 'dark' ? 'light' : 'dark'} mode`}
      aria-pressed={theme === 'dark'}
      onClick={toggle}
      className="inline-flex items-center gap-1 rounded-[var(--radius)] px-2 py-0.5 text-[12px] text-(--color-fg-secondary) hover:bg-(--color-bg-hover)"
      data-testid="theme-toggle"
    >
      {theme === 'dark' ? '🌙' : '☀️'}
      <span className="hidden lg:inline">{theme === 'dark' ? 'Dark' : 'Light'}</span>
    </button>
  );
}
