import { useEffect, useRef, useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { ChevronsDown, ChevronsRight, Crosshair, RefreshCw } from 'lucide-react';
import { useTrace } from '../../hooks/useTrace';
import { useSession } from '../../store/session';
import { ErrorBoundary } from '../ErrorBoundary';
import { ErrorCard } from '../ErrorCard';
import { Toolbar, ToolbarButton, ToolbarDivider } from '../Toolbar';

interface Entry {
  id: number;
  kind: string;
  code_address: string;
  target_address: string;
  children?: Entry[];
}

export function TracePanel() {
  return (
    <ErrorBoundary label="TracePanel">
      <TracePanelInner />
    </ErrorBoundary>
  );
}

function TracePanelInner() {
  const { data, isLoading, error, refetch } = useTrace();
  const expandTick = useSession((s) => s.traceExpandTick);
  const collapseTick = useSession((s) => s.traceCollapseTick);
  const currentId = useSession((s) => s.currentSnapshotId);
  const qc = useQueryClient();
  const containerRef = useRef<HTMLDivElement | null>(null);
  /** "force open" version pulses to expand-all; "force close" pulses to collapse-all */
  const [forceOpen, setForceOpen] = useState(0);
  const [forceClose, setForceClose] = useState(0);

  useEffect(() => {
    if (expandTick > 0) setForceOpen((n) => n + 1);
  }, [expandTick]);
  useEffect(() => {
    if (collapseTick > 0) setForceClose((n) => n + 1);
  }, [collapseTick]);

  function scrollToCurrent() {
    if (!containerRef.current) return;
    const el = containerRef.current.querySelector(`[data-testid="trace-entry-${currentId}"]`);
    if (el && 'scrollIntoView' in el) (el as HTMLElement).scrollIntoView({ block: 'center' });
  }

  if (isLoading)
    return (
      <div className="flex h-full flex-col">
        <Toolbar testid="trace-toolbar" />
        <div className="p-4 text-(--color-fg-tertiary)">Loading trace…</div>
      </div>
    );
  if (error) return <ErrorCard message={(error as Error).message} onRetry={() => refetch()} />;
  if (!data) return null;

  return (
    <div className="flex h-full flex-col">
      <Toolbar testid="trace-toolbar">
        <ToolbarButton
          icon={ChevronsDown}
          label="Expand all"
          testid="trace-expand-all"
          onClick={() => useSession.getState().bumpTraceExpand()}
        />
        <ToolbarButton
          icon={ChevronsRight}
          label="Collapse all"
          testid="trace-collapse-all"
          onClick={() => useSession.getState().bumpTraceCollapse()}
        />
        <ToolbarDivider />
        <ToolbarButton
          icon={Crosshair}
          label="Scroll to current snapshot"
          testid="trace-scroll-current"
          onClick={scrollToCurrent}
        />
        <ToolbarDivider />
        <ToolbarButton
          icon={RefreshCw}
          label="Refresh trace"
          testid="trace-refresh"
          onClick={() => qc.invalidateQueries({ queryKey: ['trace'] })}
        />
      </Toolbar>
      <div
        ref={containerRef}
        data-testid="trace-panel"
        className="flex-1 overflow-auto p-2 font-mono text-sm"
      >
        {(data as Entry[]).map((e) => (
          <TraceNode
            key={e.id}
            entry={e}
            depth={0}
            forceOpen={forceOpen}
            forceClose={forceClose}
            currentId={currentId}
          />
        ))}
      </div>
    </div>
  );
}

function TraceNode({
  entry,
  depth,
  forceOpen,
  forceClose,
  currentId,
}: {
  entry: Entry;
  depth: number;
  forceOpen: number;
  forceClose: number;
  currentId: number;
}) {
  const setId = useSession((s) => s.setSnapshotId);
  const [open, setOpen] = useState(true);

  useEffect(() => {
    if (forceOpen > 0) setOpen(true);
  }, [forceOpen]);
  useEffect(() => {
    if (forceClose > 0) setOpen(false);
  }, [forceClose]);

  const isCurrent = entry.id === currentId;
  const hasKids = (entry.children?.length ?? 0) > 0;
  return (
    <>
      <div className="flex w-full items-center" style={{ paddingLeft: `${depth * 16 + 4}px` }}>
        {hasKids ? (
          <button
            type="button"
            onClick={() => setOpen((o) => !o)}
            data-testid={`trace-toggle-${entry.id}`}
            className="mr-1 inline-flex h-4 w-4 items-center justify-center text-(--color-fg-tertiary) hover:text-(--color-fg)"
          >
            {open ? '▾' : '▸'}
          </button>
        ) : (
          <span className="mr-1 inline-block h-4 w-4" />
        )}
        <button
          type="button"
          data-testid={`trace-entry-${entry.id}`}
          onClick={() => setId(entry.id)}
          className={
            'flex-1 rounded px-2 py-0.5 text-left hover:bg-(--color-bg-hover) ' +
            (isCurrent ? 'bg-(--color-accent-dim) text-(--color-fg)' : '')
          }
        >
          <span className="text-(--color-syn-keyword)">[{entry.kind}]</span>{' '}
          <span className="text-(--color-fg-secondary)">→</span>{' '}
          <span className="text-(--color-syn-type)">{entry.target_address.slice(0, 10)}…</span>
        </button>
      </div>
      {open &&
        entry.children?.map((c) => (
          <TraceNode
            key={c.id}
            entry={c}
            depth={depth + 1}
            forceOpen={forceOpen}
            forceClose={forceClose}
            currentId={currentId}
          />
        ))}
    </>
  );
}
