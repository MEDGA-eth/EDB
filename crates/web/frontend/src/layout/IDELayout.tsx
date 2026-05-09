import { useEffect } from 'react';
import { ActivityBar } from './ActivityBar';
import { StatusBar } from './StatusBar';
import { EditorArea } from './EditorArea';
import { BottomArea } from './BottomArea';
import { FileExplorer } from '../components/explorer/FileExplorer';
import { BreakpointsView } from '../components/explorer/BreakpointsView';
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

  return (
    <div className="flex h-full flex-col bg-(--color-bg-root)" data-testid="ide-layout">
      {showEmptyBanner && (
        <div
          data-testid="empty-trace-banner"
          role="status"
          className="flex items-center justify-center border-b border-(--color-border) bg-(--color-bg-elevated) px-4 py-2 font-display text-xs text-(--color-fg-secondary)"
        >
          No trace loaded — start with{' '}
          <code className="mx-1 font-mono">edb replay --ui=web &lt;tx&gt;</code>
        </div>
      )}
      <div className="flex flex-1 overflow-hidden">
        {/* activity bar (left) */}
        <div style={{ width: ACTIVITY_WIDTH_PX }} className="shrink-0">
          <ActivityBar />
        </div>
        {/* sidebar */}
        <SideBar activity={activity} />
        {/* main split: editor (top) + bottom panel */}
        <div className="flex flex-1 flex-col overflow-hidden">
          <div className="flex-1 overflow-hidden" data-testid="editor-region">
            {/*
              The editor area stays mounted across window resizes — dockview
              observes its container and handles its own layout. Re-mounting
              on every resize was pathological under drag (60fps).
            */}
            <EditorArea />
          </div>
          <div
            className="h-2 cursor-row-resize border-t border-b border-(--color-border) bg-(--color-bg)"
            aria-hidden
          />
          <div className="h-[30%] min-h-[160px] overflow-hidden" data-testid="bottom-region">
            <BottomArea />
          </div>
        </div>
      </div>
      <StatusBar />
      <CommandPalette />
    </div>
  );
}
