import { useQueryClient } from '@tanstack/react-query';
import { useEffect, useMemo, useRef, useState } from 'react';
import { ChevronRight, FileCode2, Hash, Play, Search } from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import { useSession } from '../store/session';
import { useSnapshotCount } from '../hooks/useSnapshotCount';
import { useNextCall } from '../hooks/useNextCall';
import { usePrevCall } from '../hooks/usePrevCall';
import { useAvailableFiles } from '../hooks/useAvailableFiles';
import { COMMANDS, type Command, type CommandCtx } from '../lib/commands';
import { rankBy } from '../lib/fuzzyMatch';

const RECENT_KEY = 'edb-web:palette-recent';
const RECENT_MAX = 10;

type PaletteRow =
  | { kind: 'command'; id: string; label: string; hint?: string; group?: string; run(): void }
  | { kind: 'file'; id: string; label: string; hint?: string; run(): void }
  | { kind: 'snapshot'; id: string; label: string; hint?: string; run(): void }
  | { kind: 'address'; id: string; label: string; hint?: string; run(): void };

function loadRecent(): string[] {
  if (typeof localStorage === 'undefined') return [];
  try {
    const raw = localStorage.getItem(RECENT_KEY);
    if (!raw) return [];
    const arr = JSON.parse(raw);
    return Array.isArray(arr) ? (arr.filter((s) => typeof s === 'string') as string[]) : [];
  } catch {
    return [];
  }
}

function pushRecent(id: string) {
  if (typeof localStorage === 'undefined') return;
  const cur = loadRecent().filter((x) => x !== id);
  cur.unshift(id);
  while (cur.length > RECENT_MAX) cur.pop();
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify(cur));
  } catch {
    // ignore
  }
}

function shortAddr(a: string): string {
  return `${a.slice(0, 6)}…${a.slice(-4)}`;
}

function iconFor(kind: PaletteRow['kind']): LucideIcon {
  if (kind === 'file') return FileCode2;
  if (kind === 'snapshot') return Hash;
  if (kind === 'address') return Hash;
  return Play;
}

export function CommandPalette() {
  const open = useSession((s) => s.paletteOpen);
  if (!open) return null;
  return <CommandPaletteInner />;
}

function CommandPaletteInner() {
  const close = () => useSession.getState().setPaletteOpen(false);
  const setSnap = useSession((s) => s.setSnapshotId);
  const openFile = useSession((s) => s.openFile);
  const queryClient = useQueryClient();

  const { data: count } = useSnapshotCount();
  const snapshotCount = count ?? 0;
  const cur = useSession((s) => s.currentSnapshotId);
  const { data: nextCallId } = useNextCall(cur);
  const { data: prevCallId } = usePrevCall(cur);

  const { files, addresses } = useAvailableFiles();

  const ctx: CommandCtx = useMemo(
    () => ({ queryClient, snapshotCount, nextCallId, prevCallId }),
    [queryClient, snapshotCount, nextCallId, prevCallId],
  );

  const [query, setQuery] = useState('');
  const [activeIdx, setActiveIdx] = useState(0);
  const inputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  /* ---------- build the row set ---------- */

  const rows = useMemo<PaletteRow[]>(() => {
    const trimmed = query.trim();
    const isCommandMode = trimmed.startsWith('>');
    const isSnapshotMode = trimmed.startsWith(':') || trimmed.startsWith('#');
    const stripped = isCommandMode || isSnapshotMode ? trimmed.slice(1).trim() : trimmed;

    /* commands */
    const enabledCommands = COMMANDS.filter((c) => (c.enabled ? c.enabled(ctx) : true));
    const commandRows: PaletteRow[] = enabledCommands.map((c) => commandToRow(c, ctx));

    /* snapshots — only generate at most 50 to avoid huge lists */
    const snapshotRows: PaletteRow[] = [];
    if (snapshotCount > 0) {
      const limit = Math.min(snapshotCount, 50);
      for (let i = 0; i < limit; i += 1) {
        snapshotRows.push({
          kind: 'snapshot',
          id: `snap:${i}`,
          label: `Go to snapshot ${i}`,
          hint: `${i} / ${snapshotCount}`,
          run: () => setSnap(i),
        });
      }
    }

    /* files */
    const fileRows: PaletteRow[] = files.map((f) => ({
      kind: 'file',
      id: `file:${f.addr}::${f.path}`,
      label: f.path === '<disasm>' ? `disasm @ ${shortAddr(f.addr)}` : (f.path.split('/').pop() ?? f.path),
      hint: `${shortAddr(f.addr)} · ${f.path}`,
      run: () => openFile({ addr: f.addr, path: f.path }),
    }));

    /* addresses */
    const addressRows: PaletteRow[] = addresses.map((addr) => ({
      kind: 'address',
      id: `addr:${addr}`,
      label: shortAddr(addr),
      hint: addr,
      run: () => {
        // jumping to an address doesn't change snapshot — we open its disasm/source as fallback
        const file = files.find((f) => f.addr === addr);
        if (file) openFile({ addr: file.addr, path: file.path });
      },
    }));

    /* prefix routing */
    let pool: PaletteRow[];
    if (isCommandMode) pool = commandRows;
    else if (isSnapshotMode) pool = snapshotRows;
    else pool = [...fileRows, ...addressRows, ...commandRows];

    /* empty input → recent first */
    if (!stripped) {
      const recent = new Set(loadRecent());
      const recentRows = pool.filter((r) => recent.has(r.id));
      const rest = pool.filter((r) => !recent.has(r.id));
      return [...recentRows, ...rest].slice(0, 60);
    }

    return rankBy(pool, stripped, (r) => `${r.label} ${r.hint ?? ''}`)
      .slice(0, 80)
      .map((r) => r.item);
  }, [query, ctx, snapshotCount, files, addresses, openFile, setSnap]);

  /* keep activeIdx in range */
  useEffect(() => {
    if (activeIdx >= rows.length) setActiveIdx(0);
  }, [rows.length, activeIdx]);

  function dispatch(row: PaletteRow) {
    pushRecent(row.id);
    row.run();
    close();
  }

  function onKey(e: React.KeyboardEvent<HTMLInputElement>) {
    if (e.key === 'Escape') {
      e.preventDefault();
      close();
      return;
    }
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setActiveIdx((i) => Math.min(rows.length - 1, i + 1));
      return;
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault();
      setActiveIdx((i) => Math.max(0, i - 1));
      return;
    }
    if (e.key === 'Enter') {
      e.preventDefault();
      const row = rows[activeIdx];
      if (row) dispatch(row);
    }
  }

  return (
    <div
      role="dialog"
      aria-modal="true"
      data-testid="command-palette"
      className="fixed inset-0 z-50 flex items-start justify-center bg-(--color-bg-root)/70 pt-[15vh]"
      onClick={close}
    >
      <div
        data-testid="palette-panel"
        className="flex max-h-[70vh] w-[640px] flex-col overflow-hidden rounded-[var(--radius)] border border-(--color-border-strong) bg-(--color-bg-elevated) shadow-[var(--shadow-lg)]"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-2 border-b border-(--color-border) px-3 py-2">
          <Search size={14} className="text-(--color-fg-tertiary)" />
          <input
            ref={inputRef}
            data-testid="palette-input"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={onKey}
            placeholder="Type a file name, > for commands, : for snapshots…"
            className="flex-1 bg-transparent font-mono text-sm outline-none placeholder:text-(--color-fg-tertiary)"
          />
          <kbd className="font-mono text-[10px] text-(--color-fg-tertiary)">esc</kbd>
        </div>
        <ul data-testid="palette-list" className="flex-1 overflow-auto py-1">
          {rows.length === 0 && (
            <li className="px-4 py-6 text-center text-xs text-(--color-fg-tertiary)">
              No matches. Try typing fewer characters.
            </li>
          )}
          {rows.map((row, i) => (
            <PaletteRowView
              key={row.id}
              row={row}
              active={i === activeIdx}
              onMouseEnter={() => setActiveIdx(i)}
              onClick={() => dispatch(row)}
            />
          ))}
        </ul>
      </div>
    </div>
  );
}

function PaletteRowView({
  row,
  active,
  onMouseEnter,
  onClick,
}: {
  row: PaletteRow;
  active: boolean;
  onMouseEnter(): void;
  onClick(): void;
}) {
  const Icon = iconFor(row.kind);
  return (
    <li>
      <button
        type="button"
        onMouseEnter={onMouseEnter}
        onClick={onClick}
        data-testid={`palette-row-${row.id}`}
        data-active={active || undefined}
        className={
          'flex w-full items-center gap-3 px-3 py-1.5 text-left text-sm ' +
          (active ? 'bg-(--color-bg-active) text-(--color-fg)' : 'text-(--color-fg-secondary) hover:bg-(--color-bg-hover)')
        }
      >
        <Icon size={14} className="shrink-0 text-(--color-fg-tertiary)" />
        <span className="truncate font-mono">{row.label}</span>
        {row.hint && (
          <span className="ml-auto truncate font-mono text-[11px] text-(--color-fg-tertiary)">{row.hint}</span>
        )}
        {active && <ChevronRight size={12} className="shrink-0 text-(--color-fg-tertiary)" />}
      </button>
    </li>
  );
}

function commandToRow(c: Command, ctx: CommandCtx): PaletteRow {
  return {
    kind: 'command',
    id: `cmd:${c.id}`,
    label: c.label,
    hint: c.hint ? c.hint : c.group,
    group: c.group,
    run: () => c.run(ctx),
  };
}
