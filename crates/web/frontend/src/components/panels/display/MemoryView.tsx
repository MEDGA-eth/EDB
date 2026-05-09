import { useMemo, useState } from 'react';
import type { SnapshotInfo } from '../../../lib/types';
import { formatMemory } from './formatMemory';

/**
 * For a 16 KB memory image we'd render ~512 rows of hex; the synchronous
 * formatting can stutter paint. Default to the first 4 KB and let the user
 * opt into the full dump.
 */
const MEMORY_FORMAT_DEFAULT_BYTES = 4 * 1024;
const MEMORY_LARGE_THRESHOLD = 16 * 1024;

export function MemoryView({ snap }: { snap: SnapshotInfo | undefined }) {
  const mem = snap?.detail.kind === 'Opcode' ? snap.detail.memory : [];
  const [showAll, setShowAll] = useState(false);
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
      <pre>{formatted}</pre>
    </div>
  );
}
