import { create } from 'zustand';
import { persist, createJSONStorage } from 'zustand/middleware';
import type { Breakpoint } from '../lib/types';
import type { Theme } from '../lib/theme';

export type TerminalEntry =
  | { kind: 'input'; ts: number; text: string }
  | { kind: 'result'; ts: number; expr: string; value: unknown }
  | { kind: 'error'; ts: number; expr: string; code: number; message: string };

export type ConnectionState = 'connected' | 'degraded' | 'offline';
export type PanelTab = 'code' | 'trace' | 'display' | 'terminal';
export type ActivityKind = 'explorer' | 'trace' | 'variables' | 'terminal' | 'breakpoints';

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

  setSnapshotId(id: number): void;
  nextSnapshot(max: number): void;
  prevSnapshot(): void;
  addBreakpoint(bp: Breakpoint): void;
  removeBreakpoint(idx: number): void;
  clearBreakpoints(): void;
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

      setSnapshotId: (id) => set({ currentSnapshotId: Math.max(0, id) }),
      nextSnapshot: (max) =>
        set({ currentSnapshotId: Math.min(get().currentSnapshotId + 1, Math.max(0, max - 1)) }),
      prevSnapshot: () => set({ currentSnapshotId: Math.max(0, get().currentSnapshotId - 1) }),
      addBreakpoint: (bp) => set({ breakpoints: [...get().breakpoints, bp] }),
      removeBreakpoint: (idx) =>
        set({ breakpoints: get().breakpoints.filter((_, i) => i !== idx) }),
      clearBreakpoints: () => set({ breakpoints: [] }),
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
