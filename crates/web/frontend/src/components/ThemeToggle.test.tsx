import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ThemeToggle } from './ThemeToggle';

describe('<ThemeToggle />', () => {
  beforeEach(() => { localStorage.clear(); document.documentElement.removeAttribute('data-theme'); });
  afterEach(() => { cleanup(); });

  test('flips theme attribute on click and persists', async () => {
    render(<ThemeToggle />);
    const btn = screen.getByTestId('theme-toggle');
    const before = document.documentElement.getAttribute('data-theme');
    await userEvent.click(btn);
    const after = document.documentElement.getAttribute('data-theme');
    expect(after).not.toBe(before);
    expect(localStorage.getItem('edb-web:theme')).toBe(after);
  });

  test('aria-label reflects target mode', async () => {
    render(<ThemeToggle />);
    const btn = screen.getByTestId('theme-toggle');
    expect(btn.getAttribute('aria-label')).toMatch(/Switch to (light|dark) mode/);
  });
});
