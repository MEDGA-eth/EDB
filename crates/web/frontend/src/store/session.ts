import { create } from 'zustand';
import { persist, createJSONStorage } from 'zustand/middleware';
import type { Breakpoint } from '../lib/types';
import type { Theme } from '../lib/theme';

/**
 * Concurrent eval ordering: each user submission gets a monotonically-
 * increasing `submissionId`. The matching result/error entry inherits the
 * same id so we can pair them up even when responses land out-of-order.
 * Optional for back-compat with older persisted history.
 */
export type TerminalEntry =
  | { kind: 'input'; ts: number; text: string; submissionId?: number }
  | { kind: 'result'; ts: number; expr: string; value: unknown; submissionId?: number }
  | { kind: 'error'; ts: number; expr: string; code: number; message: string; submissionId?: number }
  // `message` is plain output from a built-in command (`bp`, `goto`, `help`, …).
  // Distinct from `result` so renderers can format them as a simple line of
  // markdown rather than a code-fenced eval value.
  | { kind: 'message'; ts: number; text: string };

export type ConnectionState = 'connected' | 'degraded' | 'offline';
export type PanelTab = 'code' | 'trace' | 'display' | 'terminal';
export type ActivityKind = 'explorer' | 'trace' | 'variables' | 'breakpoints';

/** Call-type buckets used to filter the trace tree. */
export type TraceCallFilter =
  | 'CALL'
  | 'STATICCALL'
  | 'DELEGATECALL'
  | 'CALLCODE'
  | 'CREATE'
  | 'CREATE2';

export const ALL_TRACE_CALL_FILTERS: readonly TraceCallFilter[] = [
  'CALL',
  'STATICCALL',
  'DELEGATECALL',
  'CALLCODE',
  'CREATE',
  'CREATE2',
];

export interface OpenFile {
  /** stable id used as the dockview panel id; e.g. `${addr}::${path}` */
  id: string;
  /** address-of-code this file belongs to */
  addr: string;
  /** virtual path within the bundle; for opcodes use the literal `disasm` */
  path: string;
}

export interface SessionState {
  currentSnapshotId: number;
  breakpoints: Breakpoint[];
  terminalHistory: TerminalEntry[];
  panelTab: PanelTab;
  theme: Theme;
  connection: ConnectionState;
  sessionEnded: boolean;

  /* IDE shell state */
  activeActivity: ActivityKind;
  openFiles: OpenFile[];
  activeFileId: string | null;
  /** serialised dockview JSON; persisted across reloads */
  layoutJson: string | null;

  /* command-palette + global view toggles */
  paletteOpen: boolean;
  wordWrap: boolean;
  showLineNumbers: boolean;
  /** trace expand/collapse "epoch" — bumping forces panels to re-evaluate */
  traceExpandTick: number;
  traceCollapseTick: number;
  /** persisted list of watch expressions (auto-evaluated each snapshot) */
  watchExpressions: string[];
  /** persisted set of call types to show in the trace tree. Missing → all on. */
  traceCallFilters: TraceCallFilter[];

  setSnapshotId(id: number): void;
  nextSnapshot(max: number): void;
  prevSnapshot(): void;
  addBreakpoint(bp: Breakpoint): void;
  removeBreakpoint(idx: number): void;
  clearBreakpoints(): void;
  setBreakpointCondition(idx: number, condition: string | null): void;
  setBreakpointEnabled(idx: number, enabled: boolean): void;
  enableAllBreakpoints(): void;
  disableAllBreakpoints(): void;
  appendTerminal(entry: TerminalEntry): void;
  clearTerminal(): void;
  setPanelTab(tab: PanelTab): void;
  setTheme(theme: Theme): void;
  toggleTheme(): void;
  setConnection(state: ConnectionState): void;
  setSessionEnded(ended: boolean): void;

  setActivity(a: ActivityKind): void;
  openFile(args: { addr: string; path: string }): void;
  closeFile(id: string): void;
  setActiveFile(id: string | null): void;
  setLayoutJson(json: string | null): void;

  setPaletteOpen(open: boolean): void;
  togglePalette(): void;
  setWordWrap(on: boolean): void;
  toggleWordWrap(): void;
  setShowLineNumbers(on: boolean): void;
  toggleLineNumbers(): void;
  bumpTraceExpand(): void;
  bumpTraceCollapse(): void;

  addWatchExpression(expr: string): void;
  removeWatchExpression(expr: string): void;
  clearWatchExpressions(): void;

  toggleTraceCallFilter(filter: TraceCallFilter): void;
  resetTraceCallFilters(): void;
}

function fileId(addr: string, path: string): string {
  return `${addr}::${path}`;
}

const PERSIST_KEY = 'edb-web:session';

export const useSession = create<SessionState>()(
  persist<SessionState>(
    (set, get) => ({
      currentSnapshotId: 0,
      breakpoints: [],
      terminalHistory: [],
      panelTab: 'code',
      theme: 'light',
      connection: 'connected',
      sessionEnded: false,

      activeActivity: 'explorer',
      openFiles: [],
      activeFileId: null,
      layoutJson: null,

      paletteOpen: false,
      wordWrap: false,
      showLineNumbers: true,
      traceExpandTick: 0,
      traceCollapseTick: 0,
      watchExpressions: [],
      traceCallFilters: [...ALL_TRACE_CALL_FILTERS],

      setSnapshotId: (id) => set({ currentSnapshotId: Math.max(0, id) }),
      nextSnapshot: (max) =>
        set({ currentSnapshotId: Math.min(get().currentSnapshotId + 1, Math.max(0, max - 1)) }),
      prevSnapshot: () => set({ currentSnapshotId: Math.max(0, get().currentSnapshotId - 1) }),
      addBreakpoint: (bp) =>
        set({
          breakpoints: [
            ...get().breakpoints,
            // ensure `enabled` is always populated (defaults to true) — the
            // schema accepts a missing field but the runtime needs an explicit
            // boolean so toggle semantics stay deterministic.
            { ...bp, enabled: bp.enabled ?? true },
          ],
        }),
      removeBreakpoint: (idx) =>
        set({ breakpoints: get().breakpoints.filter((_, i) => i !== idx) }),
      clearBreakpoints: () => set({ breakpoints: [] }),
      setBreakpointCondition: (idx, condition) =>
        set({
          breakpoints: get().breakpoints.map((bp, i) =>
            i === idx ? { ...bp, condition } : bp,
          ),
        }),
      setBreakpointEnabled: (idx, enabled) =>
        set({
          breakpoints: get().breakpoints.map((bp, i) =>
            i === idx ? { ...bp, enabled } : bp,
          ),
        }),
      enableAllBreakpoints: () =>
        set({ breakpoints: get().breakpoints.map((bp) => ({ ...bp, enabled: true })) }),
      disableAllBreakpoints: () =>
        set({ breakpoints: get().breakpoints.map((bp) => ({ ...bp, enabled: false })) }),
      appendTerminal: (entry) => set({ terminalHistory: [...get().terminalHistory, entry] }),
      clearTerminal: () => set({ terminalHistory: [] }),
      setPanelTab: (tab) => set({ panelTab: tab }),
      setTheme: (theme) => set({ theme }),
      toggleTheme: () => set({ theme: get().theme === 'dark' ? 'light' : 'dark' }),
      setConnection: (state) => set({ connection: state }),
      setSessionEnded: (ended) => set({ sessionEnded: ended }),

      setActivity: (a) => set({ activeActivity: a }),
      openFile: ({ addr, path }) => {
        const id = fileId(addr, path);
        const existing = get().openFiles.find((f) => f.id === id);
        if (existing) {
          set({ activeFileId: id });
          return;
        }
        set({
          openFiles: [...get().openFiles, { id, addr, path }],
          activeFileId: id,
        });
      },
      closeFile: (id) => {
        const remaining = get().openFiles.filter((f) => f.id !== id);
        const wasActive = get().activeFileId === id;
        set({
          openFiles: remaining,
          activeFileId: wasActive ? (remaining[remaining.length - 1]?.id ?? null) : get().activeFileId,
        });
      },
      setActiveFile: (id) => set({ activeFileId: id }),
      setLayoutJson: (json) => set({ layoutJson: json }),

      setPaletteOpen: (open) => set({ paletteOpen: open }),
      togglePalette: () => set({ paletteOpen: !get().paletteOpen }),
      setWordWrap: (on) => set({ wordWrap: on }),
      toggleWordWrap: () => set({ wordWrap: !get().wordWrap }),
      setShowLineNumbers: (on) => set({ showLineNumbers: on }),
      toggleLineNumbers: () => set({ showLineNumbers: !get().showLineNumbers }),
      bumpTraceExpand: () => set({ traceExpandTick: get().traceExpandTick + 1 }),
      bumpTraceCollapse: () => set({ traceCollapseTick: get().traceCollapseTick + 1 }),

      addWatchExpression: (expr) => {
        const trimmed = expr.trim();
        if (!trimmed) return;
        const cur = get().watchExpressions;
        if (cur.includes(trimmed)) return;
        set({ watchExpressions: [...cur, trimmed] });
      },
      removeWatchExpression: (expr) =>
        set({ watchExpressions: get().watchExpressions.filter((e) => e !== expr) }),
      clearWatchExpressions: () => set({ watchExpressions: [] }),

      toggleTraceCallFilter: (filter) => {
        const cur = get().traceCallFilters;
        if (cur.includes(filter)) {
          set({ traceCallFilters: cur.filter((f) => f !== filter) });
        } else {
          // Maintain canonical ordering so the persisted shape is stable.
          const next = ALL_TRACE_CALL_FILTERS.filter(
            (f) => f === filter || cur.includes(f),
          );
          set({ traceCallFilters: [...next] });
        }
      },
      resetTraceCallFilters: () =>
        set({ traceCallFilters: [...ALL_TRACE_CALL_FILTERS] }),
    }),
    {
      name: PERSIST_KEY,
      storage: createJSONStorage(() => localStorage),
      partialize: (s): SessionState => ({
        ...s,
        // ephemeral fields not persisted:
        currentSnapshotId: 0,
        breakpoints: [],
        connection: 'connected',
        sessionEnded: false,
        // per-session (don't persist):
        activeActivity: 'explorer',
        openFiles: [],
        activeFileId: null,
        paletteOpen: false,
        traceExpandTick: 0,
        traceCollapseTick: 0,
      }),
    },
  ),
);

// Cross-tab sync: when another tab writes to our persist key, rehydrate.
// This keeps theme + layout consistent across tabs without a refresh. We
// guard against `addEventListener` not existing under server-side renders.
if (typeof window !== 'undefined' && typeof window.addEventListener === 'function') {
  window.addEventListener('storage', (e: StorageEvent) => {
    if (e.key !== PERSIST_KEY) return;
    // `useSession.persist.rehydrate()` re-reads the storage and merges the
    // persisted slice into the live store. Per the zustand docs, this is
    // the canonical way to reflect external mutations to the persist key.
    void useSession.persist.rehydrate();
  });
}
