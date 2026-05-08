import type { QueryClient } from '@tanstack/react-query';
import { useSession } from '../store/session';

/**
 * The "execution context" passed to every command. We keep this small —
 * commands either mutate the zustand store (call `useSession.getState()`)
 * or reach for the QueryClient to invalidate caches.
 */
export interface CommandCtx {
  queryClient: QueryClient;
  /** total snapshot count (best-effort; commands clamp themselves) */
  snapshotCount: number;
  /** results of `usePrev/NextCall(currentSnapshotId)` if available */
  prevCallId?: number;
  nextCallId?: number;
}

export type CommandGroup =
  | 'Navigation'
  | 'View'
  | 'Trace'
  | 'Terminal'
  | 'Breakpoints'
  | 'Layout'
  | 'Help';

export interface Command {
  id: string;
  label: string;
  group: CommandGroup;
  hint?: string;
  /** when present, the palette filters this out unless the predicate returns true */
  enabled?: (ctx: CommandCtx) => boolean;
  run(ctx: CommandCtx): void;
}

/* ---------- helpers ---------- */

function setSnapshot(id: number) {
  useSession.getState().setSnapshotId(id);
}

function clamp(id: number, max: number): number {
  if (max <= 0) return 0;
  return Math.max(0, Math.min(id, max - 1));
}

/* ---------- the registry ---------- */

export const COMMANDS: Command[] = [
  // ── Navigation ──────────────────────────────────────────────
  {
    id: 'nav.next',
    label: 'Next snapshot',
    group: 'Navigation',
    hint: 'n',
    run: ({ snapshotCount }) => useSession.getState().nextSnapshot(snapshotCount),
  },
  {
    id: 'nav.prev',
    label: 'Previous snapshot',
    group: 'Navigation',
    hint: 'p',
    run: () => useSession.getState().prevSnapshot(),
  },
  {
    id: 'nav.first',
    label: 'First snapshot',
    group: 'Navigation',
    run: () => setSnapshot(0),
  },
  {
    id: 'nav.last',
    label: 'Last snapshot',
    group: 'Navigation',
    run: ({ snapshotCount }) => setSnapshot(clamp(snapshotCount - 1, snapshotCount)),
  },
  {
    id: 'nav.next-call',
    label: 'Next call',
    group: 'Navigation',
    hint: 'N',
    enabled: (c) => typeof c.nextCallId === 'number',
    run: (c) => {
      if (typeof c.nextCallId === 'number') setSnapshot(c.nextCallId);
    },
  },
  {
    id: 'nav.prev-call',
    label: 'Previous call',
    group: 'Navigation',
    hint: 'P',
    enabled: (c) => typeof c.prevCallId === 'number',
    run: (c) => {
      if (typeof c.prevCallId === 'number') setSnapshot(c.prevCallId);
    },
  },
  // ── View ────────────────────────────────────────────────────
  {
    id: 'view.toggle-theme',
    label: 'Toggle light/dark theme',
    group: 'View',
    run: () => useSession.getState().toggleTheme(),
  },
  {
    id: 'view.toggle-wrap',
    label: 'Toggle word wrap',
    group: 'View',
    run: () => useSession.getState().toggleWordWrap(),
  },
  {
    id: 'view.toggle-line-numbers',
    label: 'Toggle line numbers',
    group: 'View',
    run: () => useSession.getState().toggleLineNumbers(),
  },
  {
    id: 'view.activity.explorer',
    label: 'Show: Explorer',
    group: 'View',
    run: () => useSession.getState().setActivity('explorer'),
  },
  {
    id: 'view.activity.trace',
    label: 'Show: Trace',
    group: 'View',
    run: () => useSession.getState().setActivity('trace'),
  },
  {
    id: 'view.activity.breakpoints',
    label: 'Show: Breakpoints',
    group: 'View',
    run: () => useSession.getState().setActivity('breakpoints'),
  },
  // ── Trace ───────────────────────────────────────────────────
  {
    id: 'trace.expand-all',
    label: 'Expand all in trace',
    group: 'Trace',
    run: () => useSession.getState().bumpTraceExpand(),
  },
  {
    id: 'trace.collapse-all',
    label: 'Collapse all in trace',
    group: 'Trace',
    run: () => useSession.getState().bumpTraceCollapse(),
  },
  // ── Terminal ────────────────────────────────────────────────
  {
    id: 'terminal.clear',
    label: 'Clear terminal history',
    group: 'Terminal',
    run: () => useSession.getState().clearTerminal(),
  },
  // ── Breakpoints ─────────────────────────────────────────────
  {
    id: 'breakpoints.clear-all',
    label: 'Clear all breakpoints',
    group: 'Breakpoints',
    enabled: () => useSession.getState().breakpoints.length > 0,
    run: () => useSession.getState().clearBreakpoints(),
  },
  // ── Layout ──────────────────────────────────────────────────
  {
    id: 'layout.refresh-active',
    label: 'Refresh data for current snapshot',
    group: 'Layout',
    run: ({ queryClient }) => {
      const id = useSession.getState().currentSnapshotId;
      queryClient.invalidateQueries({ queryKey: ['snapshot', id] });
      queryClient.invalidateQueries({ queryKey: ['storage-diff', id] });
      queryClient.invalidateQueries({ queryKey: ['storage', id] });
    },
  },
  // ── Help ────────────────────────────────────────────────────
];

export function getCommand(id: string): Command | undefined {
  return COMMANDS.find((c) => c.id === id);
}
