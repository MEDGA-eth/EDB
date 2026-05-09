import { useEffect } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { getCommand, type CommandCtx } from '../lib/commands';
import { useSession } from '../store/session';

/**
 * Wires up global keyboard shortcuts.
 *
 * Palette:
 * - Cmd/Ctrl+P → toggle command palette
 * - Cmd/Ctrl+Shift+P → palette in command-mode (prefilled with `>`)
 * - Esc → close palette if open
 *
 * Debugger (VSCode-style):
 * - F5            → continue (run to next breakpoint)
 * - Shift+F5      → stop (run to end)
 * - Cmd/Ctrl+Shift+F5 → restart (jump to snapshot 0)
 * - Alt+F5        → reverse continue
 * - F10           → step over
 * - Alt+F10       → reverse step over
 * - F11           → step into (single snapshot forward)
 * - Shift+F11     → step out
 *
 * The handler is only active while mounted, so unit tests can opt-in.
 *
 * When focus is inside an editable element (CodeMirror, `<input>`,
 * `<textarea>`, or any `contenteditable`), Cmd/Ctrl+P is left alone so the
 * editor can handle it locally — e.g. CodeMirror's search panel.
 */
function isEditableTarget(target: EventTarget | null): boolean {
  if (!target || !(target instanceof Element)) return false;
  if (target.closest('.cm-editor')) return true;
  if (target.closest('input, textarea, [contenteditable="true"], [contenteditable=""]'))
    return true;
  return false;
}

interface ShortcutSpec {
  /** key matcher */
  match(e: KeyboardEvent): boolean;
  /** command id from commands.ts (or '__restart' for the inline restart action) */
  cmdId: string;
}

const SHORTCUTS: ShortcutSpec[] = [
  { match: (e) => e.key === 'F5' && !e.shiftKey && !e.altKey && !e.metaKey && !e.ctrlKey, cmdId: 'nav.continue' },
  { match: (e) => e.key === 'F5' && e.shiftKey && !e.altKey && !(e.metaKey || e.ctrlKey), cmdId: 'nav.stop' },
  { match: (e) => e.key === 'F5' && e.shiftKey && (e.metaKey || e.ctrlKey), cmdId: '__restart' },
  { match: (e) => e.key === 'F5' && e.altKey, cmdId: 'nav.reverse-continue' },
  { match: (e) => e.key === 'F10' && !e.shiftKey && !e.altKey, cmdId: 'nav.step-over' },
  { match: (e) => e.key === 'F10' && e.altKey, cmdId: 'nav.reverse-step-over' },
  { match: (e) => e.key === 'F11' && !e.shiftKey, cmdId: 'nav.next' },
  { match: (e) => e.key === 'F11' && e.shiftKey, cmdId: 'nav.step-out' },
];

export function useGlobalKeybinds(): void {
  const qc = useQueryClient();
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;

      // Cmd/Ctrl+Shift+P → palette in command mode
      if (mod && e.shiftKey && (e.key === 'p' || e.key === 'P')) {
        if (isEditableTarget(e.target) && !useSession.getState().paletteOpen) return;
        e.preventDefault();
        e.stopPropagation();
        useSession.getState().setPaletteOpen(true);
        return;
      }

      // Cmd/Ctrl+P → toggle palette
      if (mod && !e.altKey && !e.shiftKey && (e.key === 'p' || e.key === 'P')) {
        if (useSession.getState().paletteOpen) {
          e.preventDefault();
          e.stopPropagation();
          useSession.getState().togglePalette();
          return;
        }
        if (isEditableTarget(e.target)) return;
        e.preventDefault();
        e.stopPropagation();
        useSession.getState().togglePalette();
        return;
      }

      if (e.key === 'Escape' && useSession.getState().paletteOpen) {
        e.preventDefault();
        useSession.getState().setPaletteOpen(false);
        return;
      }

      // Debugger F-keys: ignore when typing in an editor / input.
      if (isEditableTarget(e.target)) return;

      for (const sc of SHORTCUTS) {
        if (sc.match(e)) {
          e.preventDefault();
          e.stopPropagation();
          // Build the minimal CommandCtx: the F-key path doesn't have call-nav
          // ids handy (they'd require a hook-level subscription), so callers
          // wanting next-call/prev-call should use the toolbar buttons or
          // palette. The hooks-driven toolbar paths still cover that case.
          // useSnapshotCount uses queryKey ['count'].
          const snapshotCount = qc.getQueryData<number>(['count']) ?? 0;
          const ctx: CommandCtx = { queryClient: qc, snapshotCount };
          if (sc.cmdId === '__restart') {
            useSession.getState().setSnapshotId(0);
            return;
          }
          if (sc.cmdId === 'nav.stop') {
            useSession.getState().setSnapshotId(Math.max(0, snapshotCount - 1));
            return;
          }
          const cmd = getCommand(sc.cmdId);
          if (!cmd) return;
          if (cmd.enabled && !cmd.enabled(ctx)) return;
          cmd.run(ctx);
          return;
        }
      }
    };
    window.addEventListener('keydown', onKey, { capture: true });
    return () => window.removeEventListener('keydown', onKey, { capture: true });
  }, [qc]);
}
