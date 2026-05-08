import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import {
  applyTheme, getStoredTheme, getSystemTheme, initialTheme, persistTheme,
} from './theme';

describe('theme', () => {
  beforeEach(() => { localStorage.clear(); document.documentElement.removeAttribute('data-theme'); });
  afterEach(() => { localStorage.clear(); });

  test('persistTheme + getStoredTheme round-trip', () => {
    persistTheme('dark');
    expect(getStoredTheme()).toBe('dark');
  });

  test('getStoredTheme returns null when nothing stored', () => {
    expect(getStoredTheme()).toBeNull();
  });

  test('getStoredTheme ignores garbage values', () => {
    localStorage.setItem('edb-web:theme', 'pink');
    expect(getStoredTheme()).toBeNull();
  });

  test('applyTheme sets data-theme attribute on <html>', () => {
    applyTheme('dark');
    expect(document.documentElement.getAttribute('data-theme')).toBe('dark');
  });

  test('initialTheme prefers stored over system', () => {
    persistTheme('light');
    expect(initialTheme()).toBe('light');
  });

  test('initialTheme falls back to system theme', () => {
    expect(['light', 'dark']).toContain(getSystemTheme());
    expect(['light', 'dark']).toContain(initialTheme());
  });
});
