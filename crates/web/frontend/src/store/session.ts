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
  /** One-shot "scroll this open file to this line" request, e.g. from a
   *  source-search result click. `nonce` bumps on every request so repeated
   *  clicks on the same (file, line) re-trigger the scroll. Ephemeral. */
  revealRequest: { fileId: string; line: number; nonce: number } | null;

  /* command-palette + global view toggles */
  paletteOpen: boolean;
  /** Optional initial query to seed the input with on next open. Used by
   *  the Cmd/Ctrl+Shift+P shortcut to land directly in command-mode (the
   *  palette interprets a leading `>` as command-only filtering). The
   *  palette consumes and clears this on mount. */
  paletteInitialQuery: string;
  wordWrap: boolean;
  showLineNumbers: boolean;
  /** Source-search regex toggle (VSCode-style). Persisted. */
  searchUseRegex: boolean;
  /** trace expand/collapse "epoch", bumping forces panels to re-evaluate */
  traceExpandTick: number;
  traceCollapseTick: number;
  /** "reveal current location" pulse. Bumped by the toolbar's
   *  "Where am I?" button so file/opcode views re-scroll to the active
   *  snapshot's line/PC even when neither the active file nor the
   *  snapshot id changed (e.g. user clicks twice in a row, or the
   *  matching tab was already open). */
  revealTick: number;
  /** Navigation history: stack of snapshot ids visited prior to the current
   *  one, most-recent last. `setSnapshotId` pushes the prior id on every
   *  change; `goBack` pops. Drives the Reverse Step button so it's a true
   *  inverse of *whatever* the user just did (step / continue / trace
   *  click / palette goto), instead of leaning on the engine's prev_id
   *  which can leap across contract boundaries. */
  navHistory: number[];
  /** persisted list of watch expressions (auto-evaluated each snapshot) */
  watchExpressions: string[];
  /** persisted set of call types to show in the trace tree. Missing → all on. */
  traceCallFilters: TraceCallFilter[];

  setSnapshotId(id: number): void;
  nextSnapshot(max: number): void;
  prevSnapshot(): void;
  /** Pop the navigation history; jumps to the previous visited snapshot
   *  without pushing the current one (so it's a true undo, not a churn). */
  goBack(): void;
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
  /** Open (or focus) a file and request a scroll to `line` once it renders. */
  openFileAtLine(args: { addr: string; path: string; line: number }): void;
  closeFile(id: string): void;
  setActiveFile(id: string | null): void;
  setLayoutJson(json: string | null): void;

  setPaletteOpen(open: boolean): void;
  togglePalette(): void;
  /** Open the palette and seed `paletteInitialQuery`. Pass `'>'` from the
   *  Cmd+Shift+P shortcut to land in command-mode. Cleared after the
   *  palette consumes it on mount. */
  openPaletteWith(initialQuery: string): void;
  consumePaletteInitialQuery(): string;
  setWordWrap(on: boolean): void;
  toggleWordWrap(): void;
  toggleSearchRegex(): void;
  setShowLineNumbers(on: boolean): void;
  toggleLineNumbers(): void;
  bumpTraceExpand(): void;
  bumpTraceCollapse(): void;
  bumpRevealTick(): void;

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
/** Cap navHistory length so a long debugging session doesn't grow it
 *  unbounded. 200 is plenty in practice — most "Reverse Step" requests
 *  unwind a handful of steps, not hundreds. */
const NAV_HISTORY_CAP = 200;

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
      revealRequest: null,

      paletteOpen: false,
      paletteInitialQuery: '',
      wordWrap: false,
      showLineNumbers: true,
      searchUseRegex: false,
      traceExpandTick: 0,
      traceCollapseTick: 0,
      revealTick: 0,
      navHistory: [],
      watchExpressions: [],
      traceCallFilters: [...ALL_TRACE_CALL_FILTERS],

      setSnapshotId: (id) => {
        const target = Math.max(0, id);
        const cur = get().currentSnapshotId;
        if (target === cur) return;
        // Push the *prior* id onto the history stack so Reverse Step can
        // undo this navigation. Capped at NAV_HISTORY_CAP entries; older
        // entries shift off the front. We only push from this single
        // setter so every navigation route — toolbar steps, palette gotos,
        // trace clicks, breakpoint hits — flows through one funnel.
        const hist = get().navHistory;
        const next = hist.length >= NAV_HISTORY_CAP ? hist.slice(1) : hist.slice();
        next.push(cur);
        set({ currentSnapshotId: target, navHistory: next });
      },
      nextSnapshot: (max) => {
        const target = Math.min(get().currentSnapshotId + 1, Math.max(0, max - 1));
        get().setSnapshotId(target);
      },
      prevSnapshot: () => {
        const target = Math.max(0, get().currentSnapshotId - 1);
        get().setSnapshotId(target);
      },
      goBack: () => {
        const hist = get().navHistory;
        if (hist.length === 0) return;
        const target = hist[hist.length - 1]!;
        // Pop *without* pushing the current id back, otherwise repeated
        // Reverse Step clicks would oscillate between two snapshots.
        set({ currentSnapshotId: target, navHistory: hist.slice(0, -1) });
      },
      addBreakpoint: (bp) =>
        set({
          breakpoints: [
            ...get().breakpoints,
            // ensure `enabled` is always populated (defaults to true), the
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
      openFileAtLine: ({ addr, path, line }) => {
        const id = fileId(addr, path);
        const exists = get().openFiles.some((f) => f.id === id);
        const prevNonce = get().revealRequest?.nonce ?? 0;
        set({
          openFiles: exists ? get().openFiles : [...get().openFiles, { id, addr, path }],
          activeFileId: id,
          revealRequest: { fileId: id, line, nonce: prevNonce + 1 },
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

      setPaletteOpen: (open) =>
        set({ paletteOpen: open, ...(open ? {} : { paletteInitialQuery: '' }) }),
      togglePalette: () => {
        const next = !get().paletteOpen;
        set({ paletteOpen: next, ...(next ? {} : { paletteInitialQuery: '' }) });
      },
      openPaletteWith: (initialQuery) =>
        set({ paletteOpen: true, paletteInitialQuery: initialQuery }),
      consumePaletteInitialQuery: () => {
        const q = get().paletteInitialQuery;
        if (q) set({ paletteInitialQuery: '' });
        return q;
      },
      setWordWrap: (on) => set({ wordWrap: on }),
      toggleWordWrap: () => set({ wordWrap: !get().wordWrap }),
      toggleSearchRegex: () => set({ searchUseRegex: !get().searchUseRegex }),
      setShowLineNumbers: (on) => set({ showLineNumbers: on }),
      toggleLineNumbers: () => set({ showLineNumbers: !get().showLineNumbers }),
      bumpTraceExpand: () => set({ traceExpandTick: get().traceExpandTick + 1 }),
      bumpTraceCollapse: () => set({ traceCollapseTick: get().traceCollapseTick + 1 }),
      bumpRevealTick: () => set({ revealTick: get().revealTick + 1 }),

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
        revealRequest: null,
        paletteOpen: false,
        paletteInitialQuery: '',
        traceExpandTick: 0,
        traceCollapseTick: 0,
        revealTick: 0,
        navHistory: [],
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
