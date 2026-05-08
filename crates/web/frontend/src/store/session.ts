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

export interface SessionState {
  currentSnapshotId: number;
  breakpoints: Breakpoint[];
  terminalHistory: TerminalEntry[];
  panelTab: PanelTab;
  theme: Theme;
  connection: ConnectionState;
  sessionEnded: boolean;

  setSnapshotId(id: number): void;
  nextSnapshot(max: number): void;
  prevSnapshot(): void;
  addBreakpoint(bp: Breakpoint): void;
  removeBreakpoint(idx: number): void;
  appendTerminal(entry: TerminalEntry): void;
  clearTerminal(): void;
  setPanelTab(tab: PanelTab): void;
  setTheme(theme: Theme): void;
  setConnection(state: ConnectionState): void;
  setSessionEnded(ended: boolean): void;
}

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

      setSnapshotId: (id) => set({ currentSnapshotId: Math.max(0, id) }),
      nextSnapshot: (max) =>
        set({ currentSnapshotId: Math.min(get().currentSnapshotId + 1, Math.max(0, max - 1)) }),
      prevSnapshot: () => set({ currentSnapshotId: Math.max(0, get().currentSnapshotId - 1) }),
      addBreakpoint: (bp) => set({ breakpoints: [...get().breakpoints, bp] }),
      removeBreakpoint: (idx) =>
        set({ breakpoints: get().breakpoints.filter((_, i) => i !== idx) }),
      appendTerminal: (entry) => set({ terminalHistory: [...get().terminalHistory, entry] }),
      clearTerminal: () => set({ terminalHistory: [] }),
      setPanelTab: (tab) => set({ panelTab: tab }),
      setTheme: (theme) => set({ theme }),
      setConnection: (state) => set({ connection: state }),
      setSessionEnded: (ended) => set({ sessionEnded: ended }),
    }),
    {
      name: 'edb-web:session',
      storage: createJSONStorage(() => localStorage),
      partialize: (s): SessionState => ({
        ...s,
        // ephemeral fields not persisted:
        currentSnapshotId: 0,
        breakpoints: [],
        connection: 'connected',
        sessionEnded: false,
      }),
    },
  ),
);
