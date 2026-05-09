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
 * Shows the return data for the active snapshot's frame. Reads the trace
 * entry for `frame_id[0]` and renders its `result.output` hex; if no
 * result is set yet (the call is still running), surfaces a status
 * placeholder rather than rendering empty.
 */
export function OutputView({ snap }: { snap: SnapshotInfo | undefined }) {
  const { data: trace } = useTrace();
  if (!snap) return <span className="text-(--color-fg-tertiary)">(no snapshot)</span>;
  const frameTraceId = traceEntryIdOf(snap.frame_id);
  const entry = trace?.inner.find((e) => e.id === frameTraceId);
  if (!entry)
    return (
      <span className="text-(--color-fg-tertiary)">
        (no trace entry for frame {frameTraceId})
      </span>
    );
  if (!entry.result)
    return (
      <div data-testid="output-view" className="flex flex-col gap-2">
        <div className="text-xs text-(--color-fg-tertiary)">(call still running)</div>
      </div>
    );

  const { display, truncated } = shortHex(entry.result.output);
  const byteLen = Math.max(0, (entry.result.output.length - 2) / 2);
  const status = entry.result.kind;

  return (
    <div data-testid="output-view" className="flex flex-col gap-2">
      <div className="flex items-center gap-2 text-xs text-(--color-fg-tertiary)">
        <span
          className={
            status === 'Success'
              ? 'text-(--color-success)'
              : 'text-(--color-danger)'
          }
        >
          {status}
        </span>
        <span>·</span>
        <span>{byteLen} byte{byteLen === 1 ? '' : 's'}</span>
        {truncated && <span>(truncated)</span>}
        <span>·</span>
        <span className="font-mono text-(--color-fg-secondary)">
          {entry.result.result}
        </span>
      </div>
      {truncated ? (
        <details>
          <summary className="cursor-pointer break-all">{display}</summary>
          <pre
            data-testid="output-full"
            className="mt-1 break-all whitespace-pre-wrap text-(--color-fg-secondary)"
          >
            {entry.result.output}
          </pre>
        </details>
      ) : (
        <span className="break-all">{display}</span>
      )}
    </div>
  );
}
