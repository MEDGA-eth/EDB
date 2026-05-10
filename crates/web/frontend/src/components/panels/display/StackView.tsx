import type { SnapshotInfo } from '../../../lib/types';

/**
 * Truncate a long hex string for display: keep the leading `0x` plus the
 * first/last few nibbles, drop the middle. Returns the input unchanged if
 * it's already short. The full value is preserved on the row's `title`
 * attribute and copied via click.
 */
function shortHex(v: string, head = 10, tail = 6): string {
  if (v.length <= head + tail + 1) return v;
  return `${v.slice(0, head)}…${v.slice(-tail)}`;
}

export function StackView({ snap }: { snap: SnapshotInfo | undefined }) {
  if (snap?.detail.kind !== 'Opcode') {
    return (
      <div className="text-(--color-fg-tertiary) italic">
        Stack is only available in opcode-level snapshots.
        Source-level (hook) snapshots do not capture the EVM stack.
      </div>
    );
  }
  const stack = snap.detail.stack;
  if (stack.length === 0)
    return <span className="text-(--color-fg-tertiary)">(empty)</span>;
  // Render top-of-stack first (index `n-1` → label `[0]` "TOS"), descending
  // toward the bottom. EVM convention: index 0 is the value most recently
  // pushed, which is what consumers care about most often.
  const rows = stack
    .map((value, originalIdx) => ({
      value,
      depth: stack.length - 1 - originalIdx,
    }))
    .sort((a, b) => a.depth - b.depth);
  return (
    <div className="flex flex-col gap-0.5" data-testid="stack-rows">
      {rows.map(({ value, depth }) => {
        const isTop = depth === 0;
        return (
          <button
            key={depth}
            type="button"
            data-testid={`stack-row-${depth}`}
            title={`Click to copy\n${value}`}
            onClick={() => {
              if (navigator.clipboard?.writeText) void navigator.clipboard.writeText(value);
            }}
            className={`grid grid-cols-[auto_auto_1fr] items-center gap-x-3 rounded px-2 py-0.5 text-left hover:bg-(--color-bg-hover) ${
              isTop ? 'bg-(--color-bg-hover)' : ''
            }`}
          >
            <span
              className={`font-mono text-xs tabular-nums ${
                isTop ? 'text-(--color-accent)' : 'text-(--color-fg-tertiary)'
              }`}
            >
              [{depth}]
            </span>
            <span
              className={`rounded px-1.5 text-[10px] font-semibold uppercase tracking-wider ${
                isTop
                  ? 'bg-(--color-accent-dim) text-(--color-accent)'
                  : 'invisible'
              }`}
            >
              TOS
            </span>
            <span className="font-mono">{shortHex(value)}</span>
          </button>
        );
      })}
    </div>
  );
}
