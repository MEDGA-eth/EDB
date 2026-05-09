import { useState } from 'react';
import { AlertTriangle, Plus, X } from 'lucide-react';
import { useSession } from '../../../store/session';
import { useWatchValue } from '../../../hooks/useWatchValue';
import { formatSolValue, type EvalResult, type SolValue } from '../../../lib/types';
import { highlightSolidity } from '../../../lib/solHighlight';
import { ErrorBoundary } from '../../ErrorBoundary';

/**
 * Render the resolved value of a watch expression. The query auto-fires on
 * snapshot change because the cache key embeds `snapshotId`. Errors come
 * back from the engine as `EvalResult.Err` (not RPC errors), so we flag
 * them inline rather than surfacing a generic "request failed" UI. Both
 * branches render with the same warning icon affordance so the error path
 * never looks like a crashed row to the user.
 */
function watchTypeLabel(v: SolValue | null): string | null {
  if (!v) return null;
  if (v.type === 'Uint') return `uint${v.value.bits}`;
  if (v.type === 'Int') return `int${v.value.bits}`;
  if (v.type === 'FixedBytes') return `bytes${v.value.size}`;
  if (v.type === 'Bool') return 'bool';
  if (v.type === 'Address') return 'address';
  if (v.type === 'String') return 'string';
  if (v.type === 'Bytes') return 'bytes';
  return v.type;
}

function WatchRow({ expr, snapshotId }: { expr: string; snapshotId: number }) {
  const { data, isLoading, error } = useWatchValue(expr, snapshotId);
  const remove = useSession((s) => s.removeWatchExpression);
  const r = (data as EvalResult | undefined) ?? null;
  const okType = r?.kind === 'Ok' ? watchTypeLabel(r.value) : null;
  const status: 'loading' | 'ok' | 'rpc-error' | 'eval-error' | 'empty' = isLoading
    ? 'loading'
    : error
      ? 'rpc-error'
      : !r
        ? 'empty'
        : r.kind === 'Err'
          ? 'eval-error'
          : 'ok';

  return (
    <li
      data-testid={`watch-row-${expr}`}
      className="rounded-md border border-(--color-border) bg-(--color-bg-elevated)/60 px-3 py-2 hover:border-(--color-border-strong) transition"
    >
      <div className="flex items-baseline justify-between gap-2">
        {/* Expression with the same Solidity highlighter the terminal uses
            so identifiers, keywords and numbers read consistently across
            both surfaces. */}
        <code
          className="break-all font-mono text-[13px]"
          title={expr}
          data-testid={`watch-expr-${expr}`}
        >
          {highlightSolidity(expr)}
        </code>
        <div className="flex items-center gap-1.5">
          {okType && (
            <span
              className="shrink-0 rounded-full px-2 py-0.5 font-mono text-[11px] tracking-wide text-(--color-syn-type-std)"
              style={{
                backgroundColor: 'var(--color-bg)',
                border: '1px solid var(--color-border)',
              }}
            >
              {okType}
            </span>
          )}
          <button
            type="button"
            title="Remove watch"
            aria-label={`Remove watch for ${expr}`}
            data-testid={`watch-remove-${expr}`}
            onClick={() => remove(expr)}
            className="flex h-5 w-5 items-center justify-center rounded text-(--color-fg-tertiary) hover:bg-(--color-bg-hover) hover:text-(--color-danger)"
          >
            <X size={14} aria-hidden />
          </button>
        </div>
      </div>
      <div className="mt-1.5 font-mono text-[13px] leading-relaxed">
        {status === 'loading' && (
          <span className="text-(--color-fg-tertiary)">evaluating…</span>
        )}
        {status === 'rpc-error' && (
          <span
            data-testid={`watch-rpc-error-${expr}`}
            className="inline-flex items-center gap-1 text-(--color-danger)"
            title={(error as Error).message}
          >
            <AlertTriangle size={14} aria-hidden /> {(error as Error).message}
          </span>
        )}
        {status === 'empty' && (
          <span className="text-(--color-fg-tertiary)">(no value)</span>
        )}
        {status === 'eval-error' && r?.kind === 'Err' && (
          <span
            data-testid={`watch-eval-error-${expr}`}
            className="inline-flex items-center gap-1 text-(--color-danger)"
            title={r.error}
          >
            <AlertTriangle size={14} aria-hidden /> {r.error}
          </span>
        )}
        {status === 'ok' && r?.kind === 'Ok' && (
          <span className="text-(--color-fg)">{formatSolValue(r.value)}</span>
        )}
      </div>
    </li>
  );
}

export function WatchView({ snapshotId }: { snapshotId: number }) {
  return (
    <ErrorBoundary label="WatchView">
      <WatchViewInner snapshotId={snapshotId} />
    </ErrorBoundary>
  );
}

function WatchViewInner({ snapshotId }: { snapshotId: number }) {
  const watches = useSession((s) => s.watchExpressions);
  const add = useSession((s) => s.addWatchExpression);
  const [draft, setDraft] = useState('');

  function submit(e: React.FormEvent) {
    e.preventDefault();
    const v = draft.trim();
    if (!v) return;
    add(v);
    setDraft('');
  }

  return (
    // No inner overflow-auto — the parent (VariablesAndWatchSidebar)
    // owns scrolling for the whole vars+watch column. Keeping a local
    // scroller would mean two scrollbars stacked, with the input box
    // pinned to the bottom of an inner box rather than the pane.
    <div data-testid="watch-view" className="flex flex-col">
      {watches.length === 0 ? (
        <p
          data-testid="watch-empty"
          className="px-1 py-2 text-[12px] text-(--color-fg-tertiary)"
        >
          No watch expressions. Add a Solidity expression below — it&apos;s
          re-evaluated on every snapshot change.
        </p>
      ) : (
        <ul className="flex flex-col gap-1.5 p-1" role="list">
          {watches.map((expr) => (
            <WatchRow key={expr} expr={expr} snapshotId={snapshotId} />
          ))}
        </ul>
      )}
      <form
        onSubmit={submit}
        className="mt-2 flex items-center gap-2 rounded border border-(--color-border) bg-(--color-bg) px-2 py-2"
      >
        <Plus size={14} className="text-(--color-fg-tertiary)" aria-hidden />
        <input
          data-testid="watch-input"
          aria-label="Add watch expression"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          placeholder="e.g. balanceOf(msg.sender)"
          className="flex-1 bg-transparent font-mono text-[13px] outline-none placeholder:text-(--color-fg-tertiary)"
        />
        <button
          type="submit"
          data-testid="watch-add"
          disabled={draft.trim().length === 0}
          className="rounded border border-(--color-border) bg-(--color-bg-elevated) px-2 py-0.5 text-[12px] text-(--color-fg-secondary) hover:bg-(--color-bg-hover) hover:text-(--color-fg) disabled:opacity-40"
        >
          Add
        </button>
      </form>
    </div>
  );
}
