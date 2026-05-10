import { useEffect, useMemo, useRef, useState } from 'react';
import { AlertTriangle, ChevronDown, ChevronRight, FileCode2, RefreshCw } from 'lucide-react';
import { useTrace } from '../../hooks/useTrace';
import { useAvailableFiles } from '../../hooks/useAvailableFiles';
import { useSession } from '../../store/session';
import { ErrorBoundary } from '../ErrorBoundary';
import { DISASM_PATH } from '../../layout/FileTabPanel';
import type { TraceEntry } from '../../lib/types';

function collectAddresses(trace: TraceEntry[] | undefined): string[] {
  if (!trace) return [];
  const out = new Set<string>();
  for (const e of trace) {
    if (e.code_address) out.add(e.code_address.toLowerCase());
  }
  return Array.from(out);
}

function shortAddr(a: string): string {
  return `${a.slice(0, 6)}…${a.slice(-4)}`;
}

export function FileExplorer() {
  return (
    <ErrorBoundary label="FileExplorer">
      <FileExplorerInner />
    </ErrorBoundary>
  );
}

/**
 * Flat row model, driven by per-address expansion state. The tree renders
 * by mapping over this list, which also determines focus order and Up/Down
 * navigation semantics.
 */
type Row =
  | { kind: 'addr'; id: string; addr: string; expanded: boolean; hasError: boolean }
  | { kind: 'file'; id: string; addr: string; path: string; label: string };

function FileExplorerInner() {
  const { data: trace, isLoading } = useTrace();
  const addresses = useMemo(() => collectAddresses(trace?.inner), [trace]);
  const { perAddress } = useAvailableFiles();
  const open = useSession((s) => s.openFile);

  // Default: every address starts expanded (preserves prior behaviour).
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  useEffect(() => {
    setExpanded((prev) => {
      const next: Record<string, boolean> = { ...prev };
      let changed = false;
      for (const a of addresses) {
        if (!(a in next)) {
          next[a] = true;
          changed = true;
        }
      }
      return changed ? next : prev;
    });
  }, [addresses]);

  // Build the visible row list.
  const rows = useMemo<Row[]>(() => {
    const list: Row[] = [];
    for (const addr of addresses) {
      const entry = perAddress.find((p) => p.addr === addr);
      const hasError = entry?.status === 'error';
      const isExpanded = expanded[addr] ?? true;
      list.push({ kind: 'addr', id: `addr:${addr}`, addr, expanded: isExpanded, hasError });
      if (isExpanded && entry && entry.status === 'ok') {
        for (const f of entry.files) {
          const label =
            f.path === DISASM_PATH ? '(disassembly)' : (f.path.split('/').pop() ?? f.path);
          list.push({
            kind: 'file',
            id: `file:${addr}::${f.path}`,
            addr,
            path: f.path,
            label,
          });
        }
      }
    }
    return list;
  }, [addresses, perAddress, expanded]);

  // Roving-tabindex: which row currently owns focus.
  const [activeId, setActiveId] = useState<string | null>(null);
  useEffect(() => {
    if (rows.length === 0) {
      setActiveId(null);
      return;
    }
    if (!activeId || !rows.some((r) => r.id === activeId)) {
      setActiveId(rows[0].id);
    }
  }, [rows, activeId]);

  // Refs for each focusable row so keyboard nav can re-focus precisely.
  const rowRefs = useRef<Record<string, HTMLDivElement | null>>({});

  function moveFocus(delta: number) {
    if (rows.length === 0) return;
    const idx = rows.findIndex((r) => r.id === activeId);
    const nextIdx = Math.max(0, Math.min(rows.length - 1, idx + delta));
    const nextRow = rows[nextIdx];
    if (nextRow) {
      setActiveId(nextRow.id);
      // Focus the DOM node directly so screen readers and visible focus track.
      requestAnimationFrame(() => rowRefs.current[nextRow.id]?.focus());
    }
  }

  function onKey(e: React.KeyboardEvent<HTMLUListElement>) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      moveFocus(1);
      return;
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault();
      moveFocus(-1);
      return;
    }
    if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') {
      const cur = rows.find((r) => r.id === activeId);
      if (cur && cur.kind === 'addr') {
        e.preventDefault();
        setExpanded((prev) => ({ ...prev, [cur.addr]: e.key === 'ArrowRight' }));
      }
      return;
    }
    if (e.key === 'Enter') {
      const cur = rows.find((r) => r.id === activeId);
      if (!cur) return;
      e.preventDefault();
      if (cur.kind === 'addr') {
        setExpanded((prev) => ({ ...prev, [cur.addr]: !prev[cur.addr] }));
      } else {
        open({ addr: cur.addr, path: cur.path });
      }
    }
  }

  if (isLoading)
    return (
      <div
        className="px-3 py-2 text-xs text-(--color-fg-tertiary)"
        data-testid="explorer-loading"
      >
        Loading…
      </div>
    );

  if (addresses.length === 0)
    return (
      <div className="px-3 py-2 text-xs text-(--color-fg-tertiary)" data-testid="explorer-empty">
        No trace yet, run <code className="font-mono">edb replay --ui=web &lt;tx&gt;</code> to load one.
      </div>
    );

  return (
    <ul
      className="font-display text-sm"
      data-testid="file-explorer"
      role="tree"
      aria-label="Contract files"
      onKeyDown={onKey}
    >
      {rows.map((row) =>
        row.kind === 'addr' ? (
          <AddressRow
            key={row.id}
            row={row}
            active={row.id === activeId}
            onActivate={() => setActiveId(row.id)}
            onToggle={() =>
              setExpanded((prev) => ({ ...prev, [row.addr]: !(prev[row.addr] ?? true) }))
            }
            onRetry={() => perAddress.find((p) => p.addr === row.addr)?.refetch()}
            errorMessage={
              perAddress.find((p) => p.addr === row.addr)?.error
            }
            registerRef={(el) => (rowRefs.current[row.id] = el)}
          />
        ) : (
          <FileRow
            key={row.id}
            row={row}
            active={row.id === activeId}
            onActivate={() => setActiveId(row.id)}
            onOpen={() => open({ addr: row.addr, path: row.path })}
            registerRef={(el) => (rowRefs.current[row.id] = el)}
          />
        ),
      )}
    </ul>
  );
}

function AddressRow({
  row,
  active,
  onActivate,
  onToggle,
  onRetry,
  errorMessage,
  registerRef,
}: {
  row: Extract<Row, { kind: 'addr' }>;
  active: boolean;
  onActivate(): void;
  onToggle(): void;
  onRetry(): void;
  errorMessage?: string;
  registerRef(el: HTMLDivElement | null): void;
}) {
  const Chevron = row.expanded ? ChevronDown : ChevronRight;
  return (
    <li className="select-none" role="treeitem" aria-expanded={row.expanded}>
      <div
        ref={registerRef}
        tabIndex={active ? 0 : -1}
        onFocus={onActivate}
        onClick={() => {
          onActivate();
          onToggle();
        }}
        className={
          'flex w-full items-center gap-1 px-2 py-1 outline-none focus:bg-(--color-bg-active) hover:bg-(--color-bg-hover) ' +
          (active ? 'bg-(--color-bg-hover)' : '')
        }
        data-testid={`explorer-addr-${row.addr}`}
        aria-label={`${row.expanded ? 'Collapse' : 'Expand'} ${shortAddr(row.addr)}`}
        role="button"
      >
        <Chevron size={12} className="text-(--color-fg-tertiary)" />
        <span className="font-mono text-xs text-(--color-syn-type)">{shortAddr(row.addr)}</span>
        {row.hasError && (
          <AlertTriangle
            size={12}
            className="ml-auto text-(--color-danger)"
            aria-label="Failed to load files for this address"
          />
        )}
      </div>
      {row.expanded && row.hasError && (
        <div
          className="flex items-center gap-2 px-6 py-1 text-xs text-(--color-danger)"
          data-testid={`explorer-error-${row.addr}`}
        >
          <span className="truncate">Failed to load: {errorMessage ?? 'unknown error'}</span>
          <button
            type="button"
            onClick={onRetry}
            data-testid={`explorer-retry-${row.addr}`}
            className="inline-flex items-center gap-1 rounded border border-(--color-border) px-1.5 py-0.5 hover:bg-(--color-bg-hover)"
            aria-label={`Retry loading files for ${shortAddr(row.addr)}`}
          >
            <RefreshCw size={10} />
            Retry
          </button>
        </div>
      )}
    </li>
  );
}

function FileRow({
  row,
  active,
  onActivate,
  onOpen,
  registerRef,
}: {
  row: Extract<Row, { kind: 'file' }>;
  active: boolean;
  onActivate(): void;
  onOpen(): void;
  registerRef(el: HTMLDivElement | null): void;
}) {
  return (
    <li role="treeitem">
      <div
        ref={registerRef}
        tabIndex={active ? 0 : -1}
        onFocus={onActivate}
        onClick={() => {
          onActivate();
          onOpen();
        }}
        className={
          'flex w-full items-center gap-2 px-2 py-1 pl-6 text-xs outline-none focus:bg-(--color-bg-active) hover:bg-(--color-bg-hover) ' +
          (active ? 'bg-(--color-bg-hover)' : '')
        }
        data-testid={`explorer-file-${row.addr}-${row.path}`}
        aria-label={`Open ${row.label}`}
        role="button"
      >
        <FileCode2 size={12} className="text-(--color-fg-tertiary)" />
        <span className="truncate">{row.label}</span>
      </div>
    </li>
  );
}
