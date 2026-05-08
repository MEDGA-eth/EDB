import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { cleanup, renderHook } from '@testing-library/react';
import { useGlobalKeybinds } from './useGlobalKeybinds';
import { useSession } from '../store/session';

function press(key: string, opts: KeyboardEventInit = {}) {
  const ev = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...opts });
  window.dispatchEvent(ev);
  return ev;
}

describe('useGlobalKeybinds', () => {
  beforeEach(() => {
    useSession.setState({ paletteOpen: false });
  });
  afterEach(cleanup);

  test('Ctrl+P opens the palette and prevents default', () => {
    renderHook(() => useGlobalKeybinds());
    const ev = press('p', { ctrlKey: true });
    expect(useSession.getState().paletteOpen).toBe(true);
    expect(ev.defaultPrevented).toBe(true);
  });

  test('Meta+P toggles back to closed when already open', () => {
    useSession.setState({ paletteOpen: true });
    renderHook(() => useGlobalKeybinds());
    press('p', { metaKey: true });
    expect(useSession.getState().paletteOpen).toBe(false);
  });

  test('Escape closes an open palette', () => {
    useSession.setState({ paletteOpen: true });
    renderHook(() => useGlobalKeybinds());
    press('Escape');
    expect(useSession.getState().paletteOpen).toBe(false);
  });

  test('plain p without modifier does nothing', () => {
    renderHook(() => useGlobalKeybinds());
    press('p');
    expect(useSession.getState().paletteOpen).toBe(false);
  });
});
