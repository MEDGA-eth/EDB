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
 *
 * When focus is inside an editable element (CodeMirror, `<input>`,
 * `<textarea>`, or any `contenteditable`), Cmd/Ctrl+P is left alone so the
 * editor can handle it locally — e.g. CodeMirror's search panel.
 */
function isEditableTarget(target: EventTarget | null): boolean {
  if (!target || !(target instanceof Element)) return false;
  // CodeMirror 6 wraps its editable surface in `.cm-editor`; descendants of
  // that subtree (the actual contenteditable div) need editor semantics.
  if (target.closest('.cm-editor')) return true;
  if (target.closest('input, textarea, [contenteditable="true"], [contenteditable=""]'))
    return true;
  return false;
}

export function useGlobalKeybinds(): void {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      if (mod && !e.altKey && (e.key === 'p' || e.key === 'P')) {
        // If the palette is already open, always toggle — even though its
        // own input is editable, the user still expects Cmd+P to close it.
        if (useSession.getState().paletteOpen) {
          e.preventDefault();
          e.stopPropagation();
          useSession.getState().togglePalette();
          return;
        }
        // Otherwise, inside an editor / input: let the local component handle it.
        if (isEditableTarget(e.target)) return;
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
