import { useCallback, useEffect, useRef, useState } from 'react';
import { ActivityBar } from './ActivityBar';
import { StatusBar } from './StatusBar';
import { DebugToolbar } from './DebugToolbar';
import { EditorArea } from './EditorArea';
import { BottomArea } from './BottomArea';
import { FileExplorer } from '../components/explorer/FileExplorer';
import { BreakpointsView } from '../components/explorer/BreakpointsView';
import { TraceSidebar } from '../components/explorer/TraceSidebar';
import { CommandPalette } from '../components/CommandPalette';
import { useGlobalKeybinds } from '../hooks/useGlobalKeybinds';
import { useSnapshotCount } from '../hooks/useSnapshotCount';
import { useTrace } from '../hooks/useTrace';
import { useSession, type ActivityKind } from '../store/session';
import { ACTIVITY_WIDTH_PX, SIDEBAR_WIDTH_PX } from '../lib/constants';

/* ── side bar ─────────────────────────────────────────────── */

function SideBar({ activity }: { activity: ActivityKind }) {
  return (
    <aside
      className="flex h-full flex-col overflow-hidden border-r border-(--color-border) bg-(--color-bg)"
      style={{ width: SIDEBAR_WIDTH_PX }}
      data-testid="side-bar"
    >
      <header className="flex h-8 items-center border-b border-(--color-border) px-3 font-display text-[11px] font-semibold tracking-wide text-(--color-fg-secondary) uppercase">
        {activity}
      </header>
      <div className="flex-1 overflow-auto" data-testid={`side-bar-${activity}`}>
        {activity === 'explorer' ? (
          <FileExplorer />
        ) : activity === 'breakpoints' ? (
          <BreakpointsView />
        ) : activity === 'trace' ? (
          <TraceSidebar />
        ) : (
          <SidePlaceholder activity={activity} />
        )}
      </div>
    </aside>
  );
}

function SidePlaceholder({ activity }: { activity: ActivityKind }) {
  return (
    <div className="px-3 py-3 text-xs text-(--color-fg-tertiary)">
      {activity} panel — coming soon.
    </div>
  );
}

/* ── shell ────────────────────────────────────────────────── */

/** Walk a trace tree and collect all `code_address` strings (lower-cased). */
function collectAddressesFromTrace(trace: unknown): Set<string> {
  const out = new Set<string>();
  if (!Array.isArray(trace)) return out;
  type Entry = { code_address?: string; children?: Entry[] };
  const walk = (e: Entry) => {
    if (typeof e.code_address === 'string') out.add(e.code_address.toLowerCase());
    e.children?.forEach(walk);
  };
  (trace as Entry[]).forEach(walk);
  return out;
}

export function IDELayout() {
  const activity = useSession((s) => s.activeActivity);
  useGlobalKeybinds();

  // Close any open file whose address is no longer in the loaded trace.
  // Keyed on `data` reference identity so we only run when the trace query
  // actually produces a new value (not on every IDELayout render).
  const trace = useTrace();
  useEffect(() => {
    const data = trace.data;
    if (!data) return;
    const addrs = collectAddressesFromTrace(data);
    const open = useSession.getState().openFiles;
    const close = useSession.getState().closeFile;
    for (const f of open) {
      if (!addrs.has(f.addr.toLowerCase())) close(f.id);
    }
  }, [trace.data]);

  // Show a top-level banner when both queries succeed but return empty data.
  // Loading states are excluded so the banner doesn't flash during cold start.
  const snapshotCountQ = useSnapshotCount();
  const traceIsEmpty = trace.isSuccess && Array.isArray(trace.data) && trace.data.length === 0;
  const noSnapshots = snapshotCountQ.isSuccess && snapshotCountQ.data === 0;
  const showEmptyBanner = traceIsEmpty && noSnapshots;

  const mainRef = useRef<HTMLDivElement | null>(null);
  // Bottom-region height as a fraction of the main split (0–1). Persisted to
  // localStorage so the user's resize sticks across reloads.
  const [bottomFrac, setBottomFrac] = useState<number>(() => {
    const raw = typeof localStorage !== 'undefined' ? localStorage.getItem('edb-web:bottom-frac') : null;
    const n = raw ? parseFloat(raw) : NaN;
    return Number.isFinite(n) && n > 0.05 && n < 0.9 ? n : 0.32;
  });

  const onSplitterDown = useCallback((startEvt: React.PointerEvent<HTMLDivElement>) => {
    const main = mainRef.current;
    if (!main) return;
    startEvt.preventDefault();
    const rect = main.getBoundingClientRect();
    const target = startEvt.currentTarget;
    target.setPointerCapture(startEvt.pointerId);
    const onMove = (e: PointerEvent) => {
      const top = e.clientY - rect.top;
      const frac = 1 - Math.min(Math.max(top / rect.height, 0.1), 0.85);
      setBottomFrac(frac);
    };
    const onUp = (e: PointerEvent) => {
      target.releasePointerCapture(e.pointerId);
      target.removeEventListener('pointermove', onMove);
      target.removeEventListener('pointerup', onUp);
      target.removeEventListener('pointercancel', onUp);
      try { localStorage.setItem('edb-web:bottom-frac', String(bottomFracRef.current)); } catch { /* ignore */ }
    };
    target.addEventListener('pointermove', onMove);
    target.addEventListener('pointerup', onUp);
    target.addEventListener('pointercancel', onUp);
  }, []);

  // Mirror current frac into a ref so onUp can persist without depending on state.
  const bottomFracRef = useRef(bottomFrac);
  useEffect(() => { bottomFracRef.current = bottomFrac; }, [bottomFrac]);

  return (
    <div className="flex h-full flex-col bg-(--color-bg-root)" data-testid="ide-layout">
      {showEmptyBanner && (
        <div
          data-testid="empty-trace-banner"
          role="status"
          className="flex items-center justify-center border-b border-(--color-border) bg-(--color-bg-elevated) px-4 py-2 font-display text-xs text-(--color-fg-secondary)"
        >
          No trace yet — run{' '}
          <code className="mx-1 font-mono">edb replay --ui=web &lt;tx&gt;</code>
          {' '}to load one.
        </div>
      )}
      <DebugToolbar />
      <div className="flex flex-1 overflow-hidden">
        {/* activity bar (left) */}
        <div style={{ width: ACTIVITY_WIDTH_PX }} className="shrink-0">
          <ActivityBar />
        </div>
        {/* sidebar */}
        <SideBar activity={activity} />
        {/* main split: editor (top) + bottom panel */}
        <div ref={mainRef} className="flex flex-1 flex-col overflow-hidden">
          <div className="overflow-hidden" data-testid="editor-region" style={{ flex: `1 1 ${(1 - bottomFrac) * 100}%`, minHeight: 80 }}>
            {/*
              The editor area stays mounted across window resizes — dockview
              observes its container and handles its own layout. Re-mounting
              on every resize was pathological under drag (60fps).
            */}
            <EditorArea />
          </div>
          <div
            role="separator"
            aria-orientation="horizontal"
            aria-label="Resize bottom panel"
            data-testid="bottom-splitter"
            onPointerDown={onSplitterDown}
            className="group relative h-1.5 shrink-0 cursor-row-resize bg-(--color-border) transition hover:bg-(--color-accent) active:bg-(--color-accent)"
          >
            <span
              aria-hidden
              className="pointer-events-none absolute inset-x-0 -top-1 -bottom-1 group-hover:bg-(--color-accent)/20"
            />
          </div>
          <div className="overflow-hidden" data-testid="bottom-region" style={{ flex: `0 0 ${bottomFrac * 100}%`, minHeight: 120 }}>
            <BottomArea />
          </div>
        </div>
      </div>
      <StatusBar />
      <CommandPalette />
    </div>
  );
}
