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
      aria-label={`Switch to ${theme === 'dark' ? 'light' : 'dark'} mode`}
      onClick={toggle}
      className="rounded-[var(--radius)] px-3 py-1 text-fg-secondary hover:bg-bg-hover"
      data-testid="theme-toggle"
    >
      {theme === 'dark' ? '🌙' : '☀️'}
    </button>
  );
}
