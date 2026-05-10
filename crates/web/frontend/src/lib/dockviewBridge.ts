import type { DockviewApi } from 'dockview';

/**
 * Module-level handle to the live DockviewApi.
 *
 * The API only exists once `MainArea`'s `<DockviewReact onReady>` fires,
 * so consumers (commands, the palette, the activity bar) can't import the
 * api directly. This bridge gives them a stable point to call into,
 * `setDockviewApi` is invoked by `MainArea` on mount/unmount, and the
 * helpers below absorb the "no api yet" case so callers don't need to
 * defensively null-check.
 */
let api: DockviewApi | null = null;

export function setDockviewApi(next: DockviewApi | null): void {
  api = next;
}

export function getDockviewApi(): DockviewApi | null {
  return api;
}

/**
 * Ensure a fixed panel (`display` / `terminal`) is present and active.
 * If the user closed it by accident, this re-adds it next to whichever
 * panel is currently visible, `referencePanel: undefined` would put it
 * in a fresh group, so we anchor against any open panel for continuity.
 *
 * Returns true on success, false if the dockview hasn't booted yet.
 */
export function ensureFixedPanel(
  id: 'display' | 'terminal',
  title: string,
): boolean {
  const a = api;
  if (!a) return false;
  let panel = a.getPanel(id);
  if (!panel) {
    // Anchor on the sibling fixed panel if it exists; otherwise the first
    // panel of any kind; otherwise no anchor (dockview makes a new group).
    const sibling = a.getPanel(id === 'display' ? 'terminal' : 'display');
    const anyPanel = sibling ?? a.panels[0];
    a.addPanel({
      id,
      component: id,
      title,
      position: anyPanel
        ? { referencePanel: anyPanel.id, direction: 'within' as const }
        : undefined,
    });
    panel = a.getPanel(id);
  }
  panel?.api.setActive();
  return true;
}
