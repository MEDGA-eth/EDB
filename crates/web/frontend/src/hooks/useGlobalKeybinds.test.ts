import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import type { ReactNode } from 'react';
import { createElement } from 'react';
import { cleanup, renderHook } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { useGlobalKeybinds } from './useGlobalKeybinds';
import { useSession } from '../store/session';

function press(key: string, opts: KeyboardEventInit = {}) {
  const ev = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...opts });
  window.dispatchEvent(ev);
  return ev;
}

const qc = new QueryClient();
const wrapper = ({ children }: { children: ReactNode }) =>
  createElement(QueryClientProvider, { client: qc, children });

describe('useGlobalKeybinds', () => {
  beforeEach(() => {
    useSession.setState({ paletteOpen: false });
  });
  afterEach(cleanup);

  test('Ctrl+P opens the palette and prevents default', () => {
    renderHook(() => useGlobalKeybinds(), { wrapper });
    const ev = press('p', { ctrlKey: true });
    expect(useSession.getState().paletteOpen).toBe(true);
    expect(ev.defaultPrevented).toBe(true);
  });

  test('Meta+P toggles back to closed when already open', () => {
    useSession.setState({ paletteOpen: true });
    renderHook(() => useGlobalKeybinds(), { wrapper });
    press('p', { metaKey: true });
    expect(useSession.getState().paletteOpen).toBe(false);
  });

  test('Escape closes an open palette', () => {
    useSession.setState({ paletteOpen: true });
    renderHook(() => useGlobalKeybinds(), { wrapper });
    press('Escape');
    expect(useSession.getState().paletteOpen).toBe(false);
  });

  test('plain p without modifier does nothing', () => {
    renderHook(() => useGlobalKeybinds(), { wrapper });
    press('p');
    expect(useSession.getState().paletteOpen).toBe(false);
  });

  test('Cmd+P inside a CodeMirror editor does NOT open the palette', () => {
    renderHook(() => useGlobalKeybinds(), { wrapper });
    // Build a fake CodeMirror surface in the DOM and dispatch from inside it.
    const editor = document.createElement('div');
    editor.className = 'cm-editor';
    const inner = document.createElement('div');
    inner.setAttribute('contenteditable', 'true');
    editor.appendChild(inner);
    document.body.appendChild(editor);
    const ev = new KeyboardEvent('keydown', {
      key: 'p',
      metaKey: true,
      bubbles: true,
      cancelable: true,
    });
    inner.dispatchEvent(ev);
    expect(useSession.getState().paletteOpen).toBe(false);
    expect(ev.defaultPrevented).toBe(false);
    document.body.removeChild(editor);
  });

  test('Cmd+P inside a plain <input> does NOT open the palette', () => {
    renderHook(() => useGlobalKeybinds(), { wrapper });
    const input = document.createElement('input');
    document.body.appendChild(input);
    const ev = new KeyboardEvent('keydown', {
      key: 'p',
      metaKey: true,
      bubbles: true,
      cancelable: true,
    });
    input.dispatchEvent(ev);
    expect(useSession.getState().paletteOpen).toBe(false);
    expect(ev.defaultPrevented).toBe(false);
    document.body.removeChild(input);
  });
});
