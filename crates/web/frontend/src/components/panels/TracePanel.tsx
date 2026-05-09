import { useEffect, useMemo, useRef, useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import {
  ChevronDown,
  ChevronRight,
  ChevronsDown,
  ChevronsRight,
  Crosshair,
  RefreshCw,
} from 'lucide-react';
import { useTrace } from '../../hooks/useTrace';
import { useSnapshotInfo } from '../../hooks/useSnapshotInfo';
import {
  useSession,
  ALL_TRACE_CALL_FILTERS,
  type TraceCallFilter,
} from '../../store/session';
import { ErrorBoundary } from '../ErrorBoundary';
import { ErrorCard } from '../ErrorCard';
import { Toolbar, ToolbarButton, ToolbarDivider } from '../Toolbar';
import { rpc } from '../../lib/rpc';
import { DISASM_PATH } from '../../layout/FileTabPanel';
import { traceEntryIdOf } from '../../lib/types';
import type { CallType, LogData, SnapshotInfo, TraceEntry } from '../../lib/types';
import { SnapshotInfo as SnapshotInfoSchema } from '../../lib/types';

interface TreeNode {
  entry: TraceEntry;
  children: TreeNode[];
}

/**
 * Rebuild a parent → children tree from the flat `TraceEntry[]` returned by
 * `edb_getTrace`. Uses the engine-assigned `id` field (also the array
 * index) to wire children to their parents in O(n).
 */
function buildTree(entries: TraceEntry[]): TreeNode[] {
  if (entries.length === 0) return [];
  const nodes = new Map<number, TreeNode>();
  for (const e of entries) nodes.set(e.id, { entry: e, children: [] });
  const roots: TreeNode[] = [];
  for (const e of entries) {
    const node = nodes.get(e.id)!;
    if (e.parent_id === null) {
      roots.push(node);
    } else {
      const parent = nodes.get(e.parent_id);
      if (parent) parent.children.push(node);
      else roots.push(node); // orphan — surface at root rather than dropping silently
    }
  }
  return roots;
}

/**
 * Drop tree nodes whose call-type label is not in `keep`. Subtrees of
 * dropped nodes also disappear, mirroring the user's expectation that
 * filtering hides "the call and its descendants".
 */
function filterTree(roots: TreeNode[], keep: Set<TraceCallFilter>): TreeNode[] {
  function recur(n: TreeNode): TreeNode | null {
    const label = callTypeLabel(n.entry.call_type) as TraceCallFilter;
    if (!keep.has(label)) return null;
    return { entry: n.entry, children: n.children.map(recur).filter(Boolean) as TreeNode[] };
  }
  return roots.map(recur).filter(Boolean) as TreeNode[];
}

/** Render a `CallType` to a short uppercase label for the trace view. */
export function callTypeLabel(ct: CallType): string {
  if (ct.kind === 'Call') {
    switch (ct.scheme) {
      case 'Call': return 'CALL';
      case 'CallCode': return 'CALLCODE';
      case 'DelegateCall': return 'DELEGATECALL';
      case 'StaticCall': return 'STATICCALL';
    }
  }
  if (typeof ct.scheme === 'string') return 'CREATE';
  if (ct.scheme && typeof ct.scheme === 'object' && 'Create2' in ct.scheme) return 'CREATE2';
  return 'CREATE';
}

/**
 * Pick a CSS color token for a `CallType` label. Mutating call schemes
 * (`DelegateCall`, `CallCode`) get the warning token to flag that they run
 * in the caller's storage context; `StaticCall` is dimmed because it can't
 * mutate state. `CREATE`/`CREATE2` use the success token. Plain `CALL`
 * keeps the default keyword color.
 */
export function callTypeColorVar(ct: CallType): string {
  if (ct.kind === 'Create') return 'var(--color-success)';
  switch (ct.scheme) {
    case 'DelegateCall':
    case 'CallCode':
      return 'var(--color-warn)';
    case 'StaticCall':
      return 'var(--color-fg-secondary)';
    case 'Call':
    default:
      return 'var(--color-syn-keyword)';
  }
}

/** True if this call's result represents a failure (revert / halt / error). */
export function isCallFailure(entry: TraceEntry): boolean {
  if (!entry.result) return false;
  return entry.result.kind === 'Revert' || entry.result.kind === 'Error';
}

function shortAddr(a: string): string {
  return a.length > 10 ? `${a.slice(0, 8)}…${a.slice(-4)}` : a;
}

function shortHex(h: string, head = 10, tail = 4): string {
  if (h.length <= head + tail + 1) return h;
  return `${h.slice(0, head)}…${h.slice(-tail)}`;
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
  const { data: currentSnap } = useSnapshotInfo(currentId);
  const currentFrameId = currentSnap ? traceEntryIdOf(currentSnap.frame_id) : null;
  const filters = useSession((s) => s.traceCallFilters);
  const toggleFilter = useSession((s) => s.toggleTraceCallFilter);
  const filterSet = useMemo(
    () => new Set<TraceCallFilter>(filters as TraceCallFilter[]),
    [filters],
  );
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

  const roots = useMemo(() => buildTree(data?.inner ?? []), [data]);
  const filteredRoots = useMemo(
    () => filterTree(roots, filterSet),
    [roots, filterSet],
  );

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
  if (error)
    return (
      <ErrorCard
        message={(error as Error).message}
        cause={error}
        onRetry={() => refetch()}
      />
    );
  if (!data) return null;

  // Keyboard nav: ↑/↓ moves focus between entry buttons; Enter selects;
  // ←/→ collapses/expands when focused on a node with children.
  function onKey(e: React.KeyboardEvent<HTMLDivElement>) {
    if (!containerRef.current) return;
    const buttons = Array.from(
      containerRef.current.querySelectorAll<HTMLButtonElement>('[data-testid^="trace-entry-"]'),
    );
    const focusIdx = buttons.findIndex((b) => b === document.activeElement);
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      const next = buttons[Math.min(buttons.length - 1, Math.max(0, focusIdx + 1))];
      next?.focus();
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      const prev = buttons[Math.max(0, focusIdx - 1)];
      prev?.focus();
    } else if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
      const cur = buttons[focusIdx];
      if (!cur) return;
      const idStr = cur.getAttribute('data-testid')?.replace('trace-entry-', '');
      if (!idStr) return;
      const toggle = containerRef.current.querySelector<HTMLButtonElement>(
        `[data-testid="trace-toggle-${idStr}"]`,
      );
      if (toggle) {
        e.preventDefault();
        toggle.click();
      }
    }
  }

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
        role="group"
        aria-label="Trace call-type filters"
        data-testid="trace-filter-bar"
        className="flex flex-wrap items-center gap-1 border-b border-(--color-border) bg-(--color-bg) px-2 py-1"
      >
        {ALL_TRACE_CALL_FILTERS.map((f) => {
          const active = filterSet.has(f);
          return (
            <button
              key={f}
              type="button"
              data-testid={`trace-filter-${f}`}
              data-active={active ? 'true' : undefined}
              aria-pressed={active}
              onClick={() => toggleFilter(f)}
              className={
                'rounded-full border px-2 py-0.5 font-mono text-[10px] tracking-wide transition ' +
                (active
                  ? 'border-(--color-accent)/60 bg-(--color-accent-dim) text-(--color-fg)'
                  : 'border-(--color-border) bg-(--color-bg-elevated) text-(--color-fg-tertiary) hover:text-(--color-fg)')
              }
            >
              {f}
            </button>
          );
        })}
      </div>
      <div
        ref={containerRef}
        data-testid="trace-panel"
        role="tree"
        aria-label="Call trace"
        onKeyDown={onKey}
        className="flex-1 overflow-auto p-2 font-mono text-sm"
      >
        {filteredRoots.map((n) => (
          <TraceNode
            key={n.entry.id}
            node={n}
            depth={0}
            forceOpen={forceOpen}
            forceClose={forceClose}
            currentId={currentId}
            currentFrameId={currentFrameId}
          />
        ))}
      </div>
    </div>
  );
}

/**
 * Render the inline events list for a single trace entry. Collapsed by
 * default (a `📡 N events` summary); on expand, each event prints its
 * topic[0] and data hex truncated. Decoding the event signature is left
 * for a future feature — we don't currently ship a 4byte database.
 */
function EventsList({ events, indent }: { events: LogData[]; indent: number }) {
  const [open, setOpen] = useState(false);
  if (events.length === 0) return null;
  return (
    <div
      className="flex w-full flex-col"
      style={{ paddingLeft: `${indent + 24}px` }}
      data-testid="trace-events"
    >
      <button
        type="button"
        data-testid="trace-events-toggle"
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
        className="text-left text-(--color-fg-tertiary) hover:text-(--color-fg)"
      >
        📡 {events.length} event{events.length === 1 ? '' : 's'}
        <span className="ml-1 text-(--color-fg-tertiary)">{open ? '▾' : '▸'}</span>
      </button>
      {open && (
        <ul className="mt-1 list-none space-y-0.5 text-xs text-(--color-fg-secondary)">
          {events.map((ev, i) => (
            <li key={i} data-testid={`trace-event-${i}`}>
              <span className="text-(--color-syn-type)">
                {ev.topics[0] ? shortHex(ev.topics[0]) : '(no topic)'}
              </span>
              <span className="ml-2">data: {shortHex(ev.data, 14, 6)}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

// TODO(perf): virtualize for traces with thousands of entries.
// Real mainnet traces are typically <500 entries; this rendering
// pattern handles them fine. Revisit with @tanstack/react-virtual
// if a pathological trace shows up.
function TraceNode({
  node,
  depth,
  forceOpen,
  forceClose,
  currentId,
  currentFrameId,
}: {
  node: TreeNode;
  depth: number;
  forceOpen: number;
  forceClose: number;
  currentId: number;
  currentFrameId: number | null;
}) {
  const setSnap = useSession((s) => s.setSnapshotId);
  const openFile = useSession((s) => s.openFile);
  const qc = useQueryClient();
  const [open, setOpen] = useState(true);

  useEffect(() => {
    if (forceOpen > 0) setOpen(true);
  }, [forceOpen]);
  useEffect(() => {
    if (forceClose > 0) setOpen(false);
  }, [forceClose]);

  const entry = node.entry;
  // Selecting a trace entry jumps to its first snapshot. The engine guarantees
  // every executed entry has a non-null first_snapshot_id; CREATE failures
  // can be null, in which case we fall back to the trace id (rare).
  const targetSnapshotId = entry.first_snapshot_id ?? entry.id;
  // Highlight by frame: an entry is "current" when the active snapshot's
  // frame_id[0] points to it. This matches focus on the running call rather
  // than just the first snapshot of each entry.
  const isCurrent = currentFrameId !== null
    ? entry.id === currentFrameId
    : targetSnapshotId === currentId;
  const hasKids = node.children.length > 0;
  const label = callTypeLabel(entry.call_type);
  const labelColor = callTypeColorVar(entry.call_type);
  const target = entry.target_label ?? shortAddr(entry.target);
  const failed = isCallFailure(entry);
  const indent = depth * 16 + 4;

  /**
   * Click handler: jump to the first snapshot for this entry AND reveal
   * the source/disasm view for the entry's `code_address`. The path comes
   * from the cached snapshot if available, else we kick off a fetch and
   * open the file once it lands. Best-effort — the snapshot navigation
   * always happens, file-open piggybacks on cache.
   */
  function handleSelect() {
    setSnap(targetSnapshotId);
    const cached = qc.getQueryData<SnapshotInfo>(['snapshot', targetSnapshotId]);
    const addr = entry.code_address;
    if (cached) {
      const path = cached.detail.kind === 'Hook' ? cached.detail.path : DISASM_PATH;
      openFile({ addr, path });
      return;
    }
    // Best-effort fetch: open file once snapshot resolves; ignore failures.
    qc.fetchQuery({
      queryKey: ['snapshot', targetSnapshotId] as const,
      queryFn: () => rpc('edb_getSnapshotInfo', SnapshotInfoSchema, [targetSnapshotId]),
      staleTime: 30_000,
    }).then(
      (snap) => {
        const path = snap.detail.kind === 'Hook' ? snap.detail.path : DISASM_PATH;
        openFile({ addr, path });
      },
      () => {
        // best-effort: opening fails silently if the snapshot fetch errors
      },
    );
  }

  return (
    <>
      <div
        className="flex w-full items-center"
        style={{ paddingLeft: `${indent}px` }}
        role="treeitem"
        aria-expanded={hasKids ? open : undefined}
        aria-selected={isCurrent}
      >
        {hasKids ? (
          <button
            type="button"
            onClick={() => setOpen((o) => !o)}
            data-testid={`trace-toggle-${entry.id}`}
            aria-label={`${open ? 'Collapse' : 'Expand'} trace entry ${entry.id}`}
            aria-expanded={open}
            className="mr-1 inline-flex h-4 w-4 items-center justify-center text-(--color-fg-tertiary) hover:text-(--color-fg)"
          >
            {open ? <ChevronDown size={14} aria-hidden /> : <ChevronRight size={14} aria-hidden />}
          </button>
        ) : (
          <span className="mr-1 inline-block h-4 w-4" />
        )}
        <button
          type="button"
          data-testid={`trace-entry-${entry.id}`}
          data-failed={failed ? 'true' : undefined}
          data-current={isCurrent ? 'true' : undefined}
          onClick={handleSelect}
          aria-label={`Go to trace entry ${entry.id} (${label})${failed ? ' — reverted/errored' : ''}`}
          aria-pressed={isCurrent}
          className={
            'flex-1 rounded px-2 py-0.5 text-left hover:bg-(--color-bg-hover) ' +
            (isCurrent ? 'bg-(--color-accent-dim) font-semibold text-(--color-fg) ' : '') +
            (failed ? ' text-(--color-danger)' : '')
          }
        >
          <span style={{ color: labelColor }}>[{label}]</span>{' '}
          <span className="text-(--color-fg-secondary)">→</span>{' '}
          <span className="text-(--color-syn-type)">{target}</span>
          {failed && (
            <span
              className="ml-1 text-(--color-danger)"
              title={`${entry.result?.kind ?? 'Failure'}: ${entry.result?.result ?? ''}`}
              aria-label="Reverted or errored"
            >
              ⚠
            </span>
          )}
        </button>
      </div>
      <EventsList events={entry.events} indent={indent} />
      {open &&
        node.children.map((c) => (
          <TraceNode
            key={c.entry.id}
            node={c}
            depth={depth + 1}
            forceOpen={forceOpen}
            forceClose={forceClose}
            currentId={currentId}
            currentFrameId={currentFrameId}
          />
        ))}
    </>
  );
}
