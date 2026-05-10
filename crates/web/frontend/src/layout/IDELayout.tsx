import { useCallback, useEffect, useRef, useState } from 'react';
import { ActivityBar } from './ActivityBar';
import { StatusBar } from './StatusBar';
import { DebugToolbar } from './DebugToolbar';
import { MainArea } from './MainArea';
import { FileExplorer } from '../components/explorer/FileExplorer';
import { BreakpointsView } from '../components/explorer/BreakpointsView';
import { TraceSidebar } from '../components/explorer/TraceSidebar';
import { CommandPalette } from '../components/CommandPalette';
import { useGlobalKeybinds } from '../hooks/useGlobalKeybinds';
import { useSnapshotCount } from '../hooks/useSnapshotCount';
import { useSnapshotFollow } from '../hooks/useSnapshotFollow';
import { useTrace } from '../hooks/useTrace';
import { useSession, type ActivityKind } from '../store/session';
import { ACTIVITY_WIDTH_PX, SIDEBAR_WIDTH_PX } from '../lib/constants';
import { VariablesAndWatchSidebar } from '../components/explorer/VariablesAndWatchSidebar';

/* ── side bar ─────────────────────────────────────────────── */

function SideBar({ activity, width }: { activity: ActivityKind; width: number }) {
  return (
    <aside
      className="flex h-full flex-col overflow-hidden border-r border-(--color-border) bg-(--color-bg)"
      style={{ width }}
      data-testid="side-bar"
    >
      <header className="flex h-9 items-center border-b border-(--color-border) px-3 font-display text-[12px] font-semibold tracking-wide text-(--color-fg-secondary) uppercase">
        {activity}
      </header>
      <div className="flex-1 overflow-auto" data-testid={`side-bar-${activity}`}>
        {activity === 'explorer' ? (
          <FileExplorer />
        ) : activity === 'breakpoints' ? (
          <BreakpointsView />
        ) : activity === 'trace' ? (
          <TraceSidebar />
        ) : activity === 'variables' ? (
          <VariablesAndWatchSidebar />
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
      {activity} panel, coming soon.
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
  // Step → open the matching code tab automatically. Lives at the IDE
  // shell so it's active regardless of which sidebar pane is showing.
  useSnapshotFollow();

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

  const shellRef = useRef<HTMLDivElement | null>(null);
  // Sidebar width in CSS pixels, drag-resizable from its right edge,
  // clamped to a usable range and persisted between reloads.
  const [sidebarWidth, setSidebarWidth] = useState<number>(() => {
    const raw = typeof localStorage !== 'undefined' ? localStorage.getItem('edb-web:sidebar-width') : null;
    const n = raw ? parseInt(raw, 10) : NaN;
    return Number.isFinite(n) && n >= 180 && n <= 640 ? n : SIDEBAR_WIDTH_PX;
  });
  const sidebarWidthRef = useRef(sidebarWidth);
  useEffect(() => { sidebarWidthRef.current = sidebarWidth; }, [sidebarWidth]);

  const onSidebarSplitterDown = useCallback((startEvt: React.PointerEvent<HTMLDivElement>) => {
    const shell = shellRef.current;
    if (!shell) return;
    startEvt.preventDefault();
    const rect = shell.getBoundingClientRect();
    const target = startEvt.currentTarget;
    target.setPointerCapture(startEvt.pointerId);
    const onMove = (e: PointerEvent) => {
      const x = e.clientX - rect.left - ACTIVITY_WIDTH_PX;
      const w = Math.min(Math.max(x, 180), Math.max(180, rect.width - 280));
      setSidebarWidth(w);
    };
    const onUp = (e: PointerEvent) => {
      target.releasePointerCapture(e.pointerId);
      target.removeEventListener('pointermove', onMove);
      target.removeEventListener('pointerup', onUp);
      target.removeEventListener('pointercancel', onUp);
      try { localStorage.setItem('edb-web:sidebar-width', String(sidebarWidthRef.current)); } catch { /* ignore */ }
    };
    target.addEventListener('pointermove', onMove);
    target.addEventListener('pointerup', onUp);
    target.addEventListener('pointercancel', onUp);
  }, []);

  return (
    <div className="flex h-full flex-col bg-(--color-bg-root)" data-testid="ide-layout">
      {showEmptyBanner && (
        <div
          data-testid="empty-trace-banner"
          role="status"
          className="flex items-center justify-center border-b border-(--color-border) bg-(--color-bg-elevated) px-4 py-2 font-display text-xs text-(--color-fg-secondary)"
        >
          No trace yet, run{' '}
          <code className="mx-1 font-mono">edb replay --ui=web &lt;tx&gt;</code>
          {' '}to load one.
        </div>
      )}
      <DebugToolbar />
      <div ref={shellRef} className="flex flex-1 overflow-hidden">
        {/* activity bar (left) */}
        <div style={{ width: ACTIVITY_WIDTH_PX }} className="shrink-0">
          <ActivityBar />
        </div>
        {/* sidebar */}
        <SideBar activity={activity} width={sidebarWidth} />
        {/* sidebar resize handle */}
        <div
          role="separator"
          aria-orientation="vertical"
          aria-label="Resize sidebar"
          data-testid="sidebar-splitter"
          onPointerDown={onSidebarSplitterDown}
          className="group relative w-1 shrink-0 cursor-col-resize bg-(--color-border) transition hover:bg-(--color-accent) active:bg-(--color-accent)"
        >
          <span aria-hidden className="pointer-events-none absolute inset-y-0 -left-1 -right-1 group-hover:bg-(--color-accent)/20" />
        </div>
        {/* unified working area: a single dockview that hosts file tabs +
            the display / terminal panels so users can drag tabs across the
            whole working area, VSCode-style. The editor / bottom split is
            now managed by dockview's own sash. */}
        <div className="flex-1 overflow-hidden">
          <MainArea />
        </div>
      </div>
      <StatusBar />
      <CommandPalette />
    </div>
  );
}
