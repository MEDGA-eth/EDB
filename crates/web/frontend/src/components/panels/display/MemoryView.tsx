import { useMemo, useState } from 'react';
import type { SnapshotInfo } from '../../../lib/types';

interface MemoryRow {
  offset: string;
  /** 4 groups of 8 bytes, hex-formatted, ready to render. */
  groups: string[];
  /** ASCII rendering: printable bytes verbatim, non-printable as `.`. */
  ascii: string;
}

function buildRows(mem: number[]): MemoryRow[] {
  const rows: MemoryRow[] = [];
  for (let i = 0; i < mem.length; i += 32) {
    const slice = mem.slice(i, i + 32);
    const groups: string[] = [];
    for (let g = 0; g < 4; g += 1) {
      const part = slice.slice(g * 8, g * 8 + 8);
      if (part.length === 0) break;
      groups.push(part.map((b) => b.toString(16).padStart(2, '0')).join(''));
    }
    let ascii = '';
    for (const b of slice) {
      ascii += b >= 0x20 && b <= 0x7e ? String.fromCharCode(b) : '.';
    }
    rows.push({
      offset: i.toString(16).padStart(6, '0'),
      groups,
      ascii,
    });
  }
  return rows;
}

/**
 * For a 16 KB memory image we'd render ~512 rows of hex; the synchronous
 * formatting can stutter paint. Default to the first 4 KB and let the user
 * opt into the full dump.
 */
const MEMORY_FORMAT_DEFAULT_BYTES = 4 * 1024;
const MEMORY_LARGE_THRESHOLD = 16 * 1024;

export function MemoryView({ snap }: { snap: SnapshotInfo | undefined }) {
  const [showAll, setShowAll] = useState(false);
  if (snap?.detail.kind !== 'Opcode') {
    return (
      <div className="text-(--color-fg-tertiary) italic">
        Memory is only available in opcode-level snapshots.
        Source-level (hook) snapshots do not capture raw memory.
      </div>
    );
  }
  const mem = snap.detail.memory;
  const isLarge = mem.length > MEMORY_LARGE_THRESHOLD;
  const visible = useMemo(() => {
    if (mem.length === 0) return [];
    if (isLarge && !showAll) return buildRows(mem.slice(0, MEMORY_FORMAT_DEFAULT_BYTES));
    return buildRows(mem);
  }, [mem, isLarge, showAll]);

  // Empty memory is normal early in execution (no MSTORE yet). Without
  // this branch the panel rendered nothing — the bug that motivated the
  // explicit placeholder. Matches the StackView "(empty)" affordance.
  if (mem.length === 0) {
    return <span className="text-(--color-fg-tertiary)">(empty)</span>;
  }

  return (
    <div>
      {isLarge && !showAll && (
        <div className="mb-2 flex items-center gap-2 text-xs text-(--color-fg-tertiary)">
          <span>
            Showing first {MEMORY_FORMAT_DEFAULT_BYTES} bytes of {mem.length}.
          </span>
          <button
            type="button"
            data-testid="memory-show-all"
            onClick={() => setShowAll(true)}
            className="rounded border border-(--color-border) px-2 py-0.5 hover:bg-(--color-bg-hover)"
          >
            Show full memory
          </button>
        </div>
      )}
      <div
        data-testid="memory-rows"
        className="grid grid-cols-[auto_1fr_auto] gap-x-3 font-mono text-[12.5px]"
      >
        {visible.map((r) => (
          <MemoryRowFragment key={r.offset} row={r} />
        ))}
      </div>
    </div>
  );
}

function MemoryRowFragment({ row }: { row: MemoryRow }) {
  return (
    <>
      <span className="text-(--color-fg-tertiary)">{row.offset}:</span>
      <span className="break-all">
        {row.groups.map((g, i) => (
          <span key={i}>
            {i > 0 ? '  ' : ''}
            {g}
          </span>
        ))}
      </span>
      <span
        className="whitespace-pre text-(--color-fg-tertiary)"
        title={row.ascii}
      >
        {row.ascii}
      </span>
    </>
  );
}
