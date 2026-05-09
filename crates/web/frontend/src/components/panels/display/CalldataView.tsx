import { useTrace } from '../../../hooks/useTrace';
import type { SnapshotInfo } from '../../../lib/types';
import { traceEntryIdOf } from '../../../lib/types';

const TRUNCATE_THRESHOLD = 200; // hex chars (~100 bytes)

function shortHex(hex: string): { display: string; truncated: boolean } {
  if (hex.length <= TRUNCATE_THRESHOLD + 2) return { display: hex, truncated: false };
  const head = hex.slice(0, 16);
  const tail = hex.slice(-16);
  return { display: `${head}…${tail}`, truncated: true };
}

/**
 * Shows the calldata for the active snapshot. Two sources of truth:
 *
 * - Opcode snapshots carry `detail.calldata` directly (the inner-frame
 *   calldata as the EVM sees it).
 * - Hook (source) snapshots don't include calldata in the snapshot detail;
 *   instead we look up the trace entry for the snapshot's `frame_id[0]`
 *   and surface its `input` field — i.e. the call's calldata at entry.
 *
 * In both cases we render the raw hex with a head/tail truncation when
 * long, and a `<details>` toggle to expand the full payload.
 */
export function CalldataView({ snap }: { snap: SnapshotInfo | undefined }) {
  const { data: trace } = useTrace();
  if (!snap)
    return <span className="text-(--color-fg-tertiary)">(no snapshot)</span>;

  const calldataHex = (() => {
    if (snap.detail.kind === 'Opcode') return snap.detail.calldata;
    // Hook snapshot: pull from trace entry input.
    const frameTraceId = traceEntryIdOf(snap.frame_id);
    const entry = trace?.inner.find((e) => e.id === frameTraceId);
    return entry?.input ?? '0x';
  })();

  const { display, truncated } = shortHex(calldataHex);
  const byteLen = Math.max(0, (calldataHex.length - 2) / 2);

  return (
    <div data-testid="calldata-view" className="flex flex-col gap-2">
      <div className="text-xs text-(--color-fg-tertiary)">
        {byteLen} byte{byteLen === 1 ? '' : 's'}
        {truncated && ' (truncated)'}
      </div>
      {truncated ? (
        <details>
          <summary className="cursor-pointer break-all">{display}</summary>
          <pre
            data-testid="calldata-full"
            className="mt-1 break-all whitespace-pre-wrap text-(--color-fg-secondary)"
          >
            {calldataHex}
          </pre>
        </details>
      ) : (
        <span className="break-all">{display}</span>
      )}
    </div>
  );
}
