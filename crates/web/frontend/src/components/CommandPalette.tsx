import { useQueryClient } from '@tanstack/react-query';
import { useEffect, useMemo, useRef, useState } from 'react';
import {
  ChevronRight,
  FileCode2,
  Hash,
  Layers,
  Eye,
  Network,
  Settings2,
  Terminal,
  Circle,
  CornerDownLeft,
  HelpCircle,
  Search,
} from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import type { CommandGroup } from '../lib/commands';
import { useSession } from '../store/session';
import { useSnapshotCount } from '../hooks/useSnapshotCount';
import { useNextCall } from '../hooks/useNextCall';
import { usePrevCall } from '../hooks/usePrevCall';
import { useAvailableFiles } from '../hooks/useAvailableFiles';
import { useDebounced } from '../hooks/useDebounced';
import { type CommandCtx } from '../lib/commands';
import { PALETTE_RECENT_LIMIT } from '../lib/constants';
import { buildRows, type PaletteRow } from './palette/buildRows';

/**
 * How long to wait after the user stops typing before rebuilding the
 * filtered row pool. Short enough to feel snappy; long enough to skip
 * the intermediate edit states when typing fast.
 */
const PALETTE_QUERY_DEBOUNCE_MS = 60;

const RECENT_KEY = 'edb-web:palette-recent';

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
  while (cur.length > PALETTE_RECENT_LIMIT) cur.pop();
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify(cur));
  } catch (e) {
    // QuotaExceededError, SecurityError (private mode), etc., match the
    // theme.ts pattern so failures aren't silent.
    // eslint-disable-next-line no-console
    console.warn('[edb-web] failed to persist palette recents', e);
  }
}

/** Map a CommandGroup to a representative Lucide icon. */
function iconForGroup(group: CommandGroup | undefined): LucideIcon {
  switch (group) {
    case 'Navigation': return CornerDownLeft;
    case 'View':       return Eye;
    case 'Trace':      return Network;
    case 'Terminal':   return Terminal;
    case 'Breakpoints':return Circle;
    case 'Layout':     return Layers;
    case 'Help':       return HelpCircle;
    default:           return Settings2;
  }
}

function iconFor(row: PaletteRow): LucideIcon {
  if (row.kind === 'file') return FileCode2;
  if (row.kind === 'snapshot') return Hash;
  if (row.kind === 'address') return Hash;
  // command
  return iconForGroup(row.group);
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

  // Seed from session.paletteInitialQuery if the palette was opened via
  // Cmd+Shift+P (which sets `>` to land in command-mode). Consumed once.
  const [query, setQuery] = useState(() =>
    useSession.getState().consumePaletteInitialQuery(),
  );
  // Debounced query, used only for the (potentially expensive) row build,
  // so the input stays synchronously responsive while typing.
  const debouncedQuery = useDebounced(query, PALETTE_QUERY_DEBOUNCE_MS);
  const [activeIdx, setActiveIdx] = useState(0);
  const inputRef = useRef<HTMLInputElement | null>(null);
  // Element that owned focus before the palette opened. Restored on close
  // so keystrokes don't leak into a backgrounded editor.
  const previouslyFocusedRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    // Capture and blur the previously-focused element, then move focus to
    // the palette input. The blur is important, without it, keystrokes
    // can race the palette open/close and bleed into the editor below.
    const prev = document.activeElement as HTMLElement | null;
    previouslyFocusedRef.current = prev && prev !== document.body ? prev : null;
    if (previouslyFocusedRef.current && typeof previouslyFocusedRef.current.blur === 'function') {
      previouslyFocusedRef.current.blur();
    }
    inputRef.current?.focus();
    return () => {
      // On close, restore focus if the captured element is still in the DOM.
      const target = previouslyFocusedRef.current;
      if (target && document.body.contains(target) && typeof target.focus === 'function') {
        target.focus();
      }
    };
  }, []);

  const rows = useMemo<PaletteRow[]>(
    () =>
      buildRows(debouncedQuery, {
        ctx,
        snapshotCount,
        files,
        addresses,
        openFile,
        setSnap,
        loadRecent,
      }),
    [debouncedQuery, ctx, snapshotCount, files, addresses, openFile, setSnap],
  );

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
            placeholder="Search files & commands"
            className="flex-1 bg-transparent font-mono text-sm outline-none placeholder:text-(--color-fg-tertiary)"
          />
          <kbd className="font-mono text-[10px] text-(--color-fg-tertiary)">esc</kbd>
        </div>
        {query.length === 0 && (
          <div
            data-testid="palette-syntax-hint"
            className="border-b border-(--color-border) px-3 py-1 font-mono text-[11px] text-(--color-fg-tertiary)"
          >
            Type <kbd>&gt;</kbd> for commands, <kbd>:</kbd> or <kbd>#</kbd> for snapshots.
          </div>
        )}
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
  const Icon = iconFor(row);
  // The label sits in the primary `--color-fg` token so it pops even on
  // an inactive row; the hint stays in `--color-fg-tertiary` so the eye
  // can scan labels without the hint competing.
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
          (active ? 'bg-(--color-bg-active)' : 'hover:bg-(--color-bg-hover)')
        }
      >
        <Icon size={14} className="shrink-0 text-(--color-fg-tertiary)" />
        <span className={`truncate ${row.kind === 'address' ? 'font-mono' : 'font-display'} text-(--color-fg)`}>
          {row.label}
        </span>
        {row.hint && (
          <span className="ml-auto truncate font-mono text-[11px] text-(--color-fg-tertiary)">
            {row.kind === 'snapshot' ? `Navigation · ${row.hint}` : row.hint}
          </span>
        )}
        {active && <ChevronRight size={12} className="shrink-0 text-(--color-fg-tertiary)" />}
      </button>
    </li>
  );
}
