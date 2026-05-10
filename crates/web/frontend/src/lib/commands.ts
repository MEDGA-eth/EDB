import type { QueryClient } from '@tanstack/react-query';
import { z } from 'zod';
import { useSession } from '../store/session';
import { rpc } from './rpc';
import { breakpointToWire, type Breakpoint, type SnapshotInfo } from './types';
import { ensureFixedPanel } from './dockviewBridge';

const HitsSchema = z.array(z.number().int().nonnegative());

/**
 * Stable hash that mirrors `useBreakpointHits.stableHash` so cached
 * `bp-hits` queries are reused. Excludes the `enabled` field, toggling
 * a breakpoint should not refetch hits, only gate consumption.
 */
function bpHashKey(bp: Breakpoint): string {
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  const { enabled: _omit, ...rest } = bp;
  return JSON.stringify(rest, Object.keys(rest).sort());
}

async function gatherEnabledHits(qc: QueryClient): Promise<number[]> {
  const bps = useSession.getState().breakpoints.filter((bp) => bp.enabled !== false);
  const all: number[] = [];
  for (const bp of bps) {
    const key = ['bp-hits', bpHashKey(bp)] as const;
    // Prefer cache so a hotkey press doesn't pay another roundtrip when
    // useBreakpointHits has already loaded the same query. Fall through
    // to fetchQuery only on a true cache miss.
    const cached = qc.getQueryData<number[]>(key);
    if (cached) {
      for (const h of cached) all.push(h);
      continue;
    }
    try {
      const hits = await qc.fetchQuery({
        queryKey: key,
        queryFn: () => rpc('edb_getBreakpointHits', HitsSchema, [breakpointToWire(bp)]),
        staleTime: 30_000,
      });
      for (const h of hits) all.push(h);
    } catch {
      // best-effort: a single failed breakpoint shouldn't block continue
    }
  }
  return all;
}

/**
 * The "execution context" passed to every command. We keep this small,
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

/* ---------- step helpers (read from query cache only) ---------- */

/**
 * Hard cap on the number of snapshot lookups a single step-out walk will
 * perform. Steps out by walking next/prev_id one snapshot at a time and
 * checking whether the trace-entry id (frame_id[0]) has changed; the cap
 * defends against the pathological case where the cache is sparse and we
 * silently spin.
 */
const STEP_OUT_HOP_CAP = 200;

function getCachedSnapshot(qc: QueryClient, id: number): SnapshotInfo | undefined {
  return qc.getQueryData<SnapshotInfo>(['snapshot', id]);
}

/**
 * Walk the snapshot chain in `direction` until we find a snapshot whose
 * trace-entry id (`frame_id[0]`) differs from `currentFrameId`. Returns
 * `undefined` if the chain runs out (top frame), the cache misses, or
 * we hit the hop cap.
 */
function walkUntilFrameChange(
  qc: QueryClient,
  startId: number,
  direction: 'next' | 'prev',
  currentFrameId: number,
): number | undefined {
  let cur = startId;
  for (let hops = 0; hops < STEP_OUT_HOP_CAP; hops += 1) {
    const snap = getCachedSnapshot(qc, cur);
    if (!snap) return undefined;
    const nextId = direction === 'next' ? snap.next_id : snap.prev_id;
    if (nextId === cur) return undefined; // self-edge → at boundary
    const nextSnap = getCachedSnapshot(qc, nextId);
    if (!nextSnap) return undefined;
    if (nextSnap.frame_id[0] !== currentFrameId) return nextId;
    cur = nextId;
  }
  return undefined;
}

/**
 * Predicate: at-least one cached neighbour exists in `direction`. We
 * call this from `enabled` so the palette doesn't list a step-* command
 * when there's nothing to step to.
 */
function hasNeighbour(qc: QueryClient, id: number, direction: 'next' | 'prev'): boolean {
  const snap = getCachedSnapshot(qc, id);
  if (!snap) return false;
  const nb = direction === 'next' ? snap.next_id : snap.prev_id;
  return nb !== id;
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
    hint: 'F11',
    enabled: ({ snapshotCount }) => useSession.getState().currentSnapshotId < snapshotCount - 1,
    run: ({ snapshotCount }) => useSession.getState().nextSnapshot(snapshotCount),
  },
  {
    id: 'nav.prev',
    label: 'Previous snapshot',
    group: 'Navigation',
    hint: '⇧F11',
    enabled: () => useSession.getState().currentSnapshotId > 0,
    run: () => useSession.getState().prevSnapshot(),
  },
  {
    id: 'nav.first',
    label: 'First snapshot',
    group: 'Navigation',
    enabled: ({ snapshotCount }) =>
      snapshotCount > 0 && useSession.getState().currentSnapshotId !== 0,
    run: () => setSnapshot(0),
  },
  {
    id: 'nav.last',
    label: 'Last snapshot',
    group: 'Navigation',
    enabled: ({ snapshotCount }) =>
      snapshotCount > 0 && useSession.getState().currentSnapshotId !== snapshotCount - 1,
    run: ({ snapshotCount }) => setSnapshot(clamp(snapshotCount - 1, snapshotCount)),
  },
  {
    id: 'nav.step-over',
    label: 'Step over',
    group: 'Navigation',
    hint: 'F10',
    enabled: ({ queryClient }) =>
      hasNeighbour(queryClient, useSession.getState().currentSnapshotId, 'next'),
    run: ({ queryClient }) => {
      const cur = useSession.getState().currentSnapshotId;
      const snap = getCachedSnapshot(queryClient, cur);
      const target = snap ? snap.next_id : cur + 1;
      setSnapshot(target);
    },
  },
  {
    id: 'nav.step-out',
    label: 'Step out',
    group: 'Navigation',
    hint: '⇧F11',
    enabled: ({ queryClient }) => {
      const cur = useSession.getState().currentSnapshotId;
      const snap = getCachedSnapshot(queryClient, cur);
      if (!snap) return false;
      // Disabled at the top frame, nowhere to step out to.
      return snap.next_id !== cur;
    },
    run: ({ queryClient }) => {
      const cur = useSession.getState().currentSnapshotId;
      const snap = getCachedSnapshot(queryClient, cur);
      if (!snap) return;
      const target = walkUntilFrameChange(queryClient, cur, 'next', snap.frame_id[0]);
      if (typeof target === 'number') setSnapshot(target);
    },
  },
  {
    id: 'nav.go-back',
    label: 'Go back (reverse step)',
    group: 'Navigation',
    hint: '⌥F10',
    // Pure history pop — independent of the engine's prev_id, so it
    // undoes the *user's* last navigation regardless of whether that
    // was a step, a continue, a reverse-continue, or a click in the
    // trace tree. Disabled when the stack is empty.
    enabled: () => useSession.getState().navHistory.length > 0,
    run: () => useSession.getState().goBack(),
  },
  {
    id: 'nav.reverse-step-out',
    label: 'Reverse step out',
    group: 'Navigation',
    hint: '⌥⇧F11',
    enabled: ({ queryClient }) => {
      const cur = useSession.getState().currentSnapshotId;
      if (cur === 0) return false;
      const snap = getCachedSnapshot(queryClient, cur);
      if (!snap) return false;
      return snap.prev_id !== cur;
    },
    run: ({ queryClient }) => {
      const cur = useSession.getState().currentSnapshotId;
      const snap = getCachedSnapshot(queryClient, cur);
      if (!snap) return;
      const target = walkUntilFrameChange(queryClient, cur, 'prev', snap.frame_id[0]);
      if (typeof target === 'number') setSnapshot(target);
    },
  },
  {
    id: 'nav.continue',
    label: 'Continue (run to next breakpoint)',
    group: 'Navigation',
    hint: 'F5',
    enabled: ({ snapshotCount }) =>
      snapshotCount > 0 && useSession.getState().currentSnapshotId < snapshotCount - 1,
    run: async ({ queryClient, snapshotCount }) => {
      const cur = useSession.getState().currentSnapshotId;
      const hits = await gatherEnabledHits(queryClient);
      const forward = hits.filter((h) => h > cur);
      if (forward.length > 0) {
        setSnapshot(Math.min(...forward));
      } else if (snapshotCount > 0) {
        setSnapshot(snapshotCount - 1);
      }
    },
  },
  {
    id: 'nav.reverse-continue',
    label: 'Reverse continue (run back to previous breakpoint)',
    group: 'Navigation',
    hint: '⌥F5',
    enabled: () => useSession.getState().currentSnapshotId > 0,
    run: async ({ queryClient }) => {
      const cur = useSession.getState().currentSnapshotId;
      const hits = await gatherEnabledHits(queryClient);
      const backward = hits.filter((h) => h < cur);
      if (backward.length > 0) {
        setSnapshot(Math.max(...backward));
      } else {
        setSnapshot(0);
      }
    },
  },
  {
    id: 'nav.next-call',
    label: 'Next call',
    group: 'Navigation',
    hint: '⌥→',
    enabled: (c) =>
      typeof c.nextCallId === 'number' && c.nextCallId !== useSession.getState().currentSnapshotId,
    run: (c) => {
      if (typeof c.nextCallId === 'number') setSnapshot(c.nextCallId);
    },
  },
  {
    id: 'nav.prev-call',
    label: 'Previous call',
    group: 'Navigation',
    hint: '⌥←',
    enabled: (c) =>
      typeof c.prevCallId === 'number' && c.prevCallId !== useSession.getState().currentSnapshotId,
    run: (c) => {
      if (typeof c.prevCallId === 'number') setSnapshot(c.prevCallId);
    },
  },
  {
    id: 'nav.restart',
    label: 'Restart (jump to first snapshot)',
    group: 'Navigation',
    hint: '⇧⌘F5',
    enabled: () => useSession.getState().currentSnapshotId !== 0,
    run: () => setSnapshot(0),
  },
  {
    id: 'nav.stop',
    label: 'Stop (run to end)',
    group: 'Navigation',
    hint: '⇧F5',
    enabled: ({ snapshotCount }) =>
      snapshotCount > 0 && useSession.getState().currentSnapshotId !== snapshotCount - 1,
    run: ({ snapshotCount }) => setSnapshot(clamp(snapshotCount - 1, snapshotCount)),
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
  // Re-open closed dockview panels. dockview removes a panel from the
  // layout entirely when the user closes its tab, so the only way back
  // is to addPanel a fresh one. ensureFixedPanel anchors the new panel
  // to whichever sibling is still mounted so the user gets continuity.
  {
    id: 'view.panel.display',
    label: 'Reopen: Display panel',
    group: 'View',
    hint: 'recover',
    run: () => { ensureFixedPanel('display', 'Display'); },
  },
  {
    id: 'view.panel.terminal',
    label: 'Reopen: Terminal panel',
    group: 'View',
    hint: 'recover',
    run: () => { ensureFixedPanel('terminal', 'Terminal'); },
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
