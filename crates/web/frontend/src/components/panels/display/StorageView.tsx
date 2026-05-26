import { useStorageDiff } from '../../../hooks/useStorageDiff';
import { storageDiffRows } from '../../../lib/types';

/** `0xABCD…1234` short form for a long hex word; full value stays in `title`. */
function shortHex(v: string): string {
  if (v.length <= 18) return v;
  return `${v.slice(0, 10)}…${v.slice(-6)}`;
}

/**
 * Storage changes for the current snapshot, rendered as a labelled list that
 * mirrors the Variables view: a bordered card, one row per changed slot, with
 * the slot key on top and a `before → after` diff beneath (red strike-through
 * for the old value, green for the new). Long hex words are clipped with the
 * full value available on hover.
 */
export function StorageView({ id }: { id: number }) {
  const { data, error } = useStorageDiff(id);
  if (error)
    return <span className="text-(--color-danger)">{(error as Error).message}</span>;
  if (!data) return <span className="text-(--color-fg-tertiary)">Loading…</span>;
  // Drop slots that didn't change, the engine sometimes echoes touched-but-
  // unchanged slots, and rendering them with red strikethrough → green text
  // visually implies a mutation that didn't happen.
  const rows = storageDiffRows(data).filter((d) => d.before !== d.after);
  if (rows.length === 0)
    return (
      <div className="rounded border border-dashed border-(--color-border) px-3 py-2 text-[12px] italic text-(--color-fg-tertiary)">
        No storage changed at this snapshot (relative to the start of the
        transaction).
      </div>
    );
  return (
    <ul
      className="flex flex-col rounded-md border border-(--color-border) bg-(--color-bg-elevated)/40"
      role="list"
      data-testid="storage-view"
    >
      {rows.map((d, i) => (
        <li
          key={d.slot}
          data-testid={`storage-row-${i}`}
          className={`flex flex-col gap-1 px-3 py-2 transition hover:bg-(--color-bg-hover)/40 ${
            i === 0 ? '' : 'border-t border-dashed border-(--color-border)'
          }`}
        >
          <div className="flex items-baseline gap-2 min-w-0">
            <span className="shrink-0 rounded-full border border-(--color-border) bg-(--color-bg) px-2 py-0.5 font-mono text-[10.5px] tracking-wide text-(--color-fg-tertiary)">
              slot
            </span>
            <span
              className="min-w-0 flex-1 truncate font-mono text-[12px] font-semibold text-(--color-fg)"
              title={d.slot}
            >
              {shortHex(d.slot)}
            </span>
          </div>
          <div className="flex items-center gap-2 font-mono text-[12px] leading-relaxed">
            <span
              className="min-w-0 truncate text-(--color-danger) line-through"
              title={d.before}
            >
              {shortHex(d.before)}
            </span>
            <span aria-hidden className="shrink-0 text-(--color-fg-tertiary)">
              →
            </span>
            <span
              className="min-w-0 truncate text-(--color-success)"
              title={d.after}
            >
              {shortHex(d.after)}
            </span>
          </div>
        </li>
      ))}
    </ul>
  );
}
