import { useEffect } from 'react';
import { useSession } from '../store/session';

/**
 * Wires up global keyboard shortcuts.
 *
 * - Cmd/Ctrl+P → toggle command palette (also intercepts the browser print dialog)
 * - Cmd/Ctrl+Shift+P → command-mode palette (prefilled with `>`)
 * - Esc → close palette if open
 *
 * The handler is only active while mounted, so unit tests can opt-in.
 */
export function useGlobalKeybinds(): void {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      // ignore inside contenteditable / inputs for typing keys, but Cmd+P
      // is a global shortcut that always wins.
      if (mod && !e.altKey && (e.key === 'p' || e.key === 'P')) {
        e.preventDefault();
        e.stopPropagation();
        useSession.getState().togglePalette();
        return;
      }
      if (e.key === 'Escape' && useSession.getState().paletteOpen) {
        e.preventDefault();
        useSession.getState().setPaletteOpen(false);
      }
    };
    window.addEventListener('keydown', onKey, { capture: true });
    return () => window.removeEventListener('keydown', onKey, { capture: true });
  }, []);
}
