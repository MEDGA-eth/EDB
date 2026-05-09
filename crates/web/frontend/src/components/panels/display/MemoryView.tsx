import { useMemo, useState } from 'react';
import type { SnapshotInfo } from '../../../lib/types';
import { formatMemory } from './formatMemory';

/**
 * Render `formatMemory` output as two grid columns (offset, hex). Falls
 * back to a single `<pre>` if the output isn't recognisable as
 * `XXXXXX: hexbytes` rows.
 */
function MemoryGrid({ formatted }: { formatted: string }) {
  if (!formatted) return null;
  const lines = formatted.split('\n');
  const rows: { offset: string; hex: string }[] = [];
  for (const line of lines) {
    const ix = line.indexOf(': ');
    if (ix < 0) {
      // Non-conforming line — bail to plain pre to avoid mangling output.
      return <pre>{formatted}</pre>;
    }
    rows.push({ offset: line.slice(0, ix), hex: line.slice(ix + 2) });
  }
  return (
    <div className="grid grid-cols-[auto_1fr] gap-x-3 font-mono">
      {rows.map((r, i) => (
        <FragmentRow key={i} offset={r.offset} hex={r.hex} />
      ))}
    </div>
  );
}

function FragmentRow({ offset, hex }: { offset: string; hex: string }) {
  return (
    <>
      <span className="text-(--color-fg-tertiary)">{offset}:</span>
      <span className="break-all">{hex}</span>
    </>
  );
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
  const mem = snap?.detail.kind === 'Opcode' ? snap.detail.memory : [];
  const isLarge = mem.length > MEMORY_LARGE_THRESHOLD;
  // Stable identity for memoization: array reference + length covers both
  // "same snapshot" (reference equal) and "snapshot changed" (length-change
  // common, reference change definitive).
  const formatted = useMemo(() => {
    if (mem.length === 0) return '';
    if (isLarge && !showAll) {
      return formatMemory(mem.slice(0, MEMORY_FORMAT_DEFAULT_BYTES));
    }
    return formatMemory(mem);
  }, [mem, isLarge, showAll]);
  if (snap?.detail.kind !== 'Opcode') {
    return (
      <div className="text-(--color-fg-tertiary) italic">
        Memory is only available in opcode-level snapshots.
        Source-level (hook) snapshots do not capture raw memory.
      </div>
    );
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
      <MemoryGrid formatted={formatted} />
    </div>
  );
}
