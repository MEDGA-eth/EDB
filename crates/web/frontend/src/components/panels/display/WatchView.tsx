import { useState } from 'react';
import { AlertTriangle, Plus, X } from 'lucide-react';
import { useSession } from '../../../store/session';
import { useWatchValue } from '../../../hooks/useWatchValue';
import { formatSolValue, type EvalResult } from '../../../lib/types';
import { ErrorBoundary } from '../../ErrorBoundary';

/**
 * Render the resolved value of a watch expression. The query auto-fires on
 * snapshot change because the cache key embeds `snapshotId`. Errors come
 * back from the engine as `EvalResult.Err` (not RPC errors), so we flag
 * them inline rather than surfacing a generic "request failed" UI. Both
 * branches render with the same warning icon affordance so the error path
 * never looks like a crashed row to the user.
 */
function WatchRow({ expr, snapshotId }: { expr: string; snapshotId: number }) {
  const { data, isLoading, error } = useWatchValue(expr, snapshotId);
  const remove = useSession((s) => s.removeWatchExpression);

  function renderError(message: string, testid: string) {
    return (
      <span
        data-testid={testid}
        title={message}
        className="inline-flex items-center gap-1 text-(--color-danger)"
      >
        <AlertTriangle size={12} aria-hidden />
        <span className="truncate">{message}</span>
      </span>
    );
  }

  function renderValue() {
    if (isLoading) return <span className="text-(--color-fg-tertiary)">…</span>;
    if (error) return renderError((error as Error).message, `watch-rpc-error-${expr}`);
    if (!data) return <span className="text-(--color-fg-tertiary)">(no value)</span>;
    const r = data as EvalResult;
    if (r.kind === 'Err') return renderError(r.error, `watch-eval-error-${expr}`);
    return <span className="text-(--color-fg)">{formatSolValue(r.value)}</span>;
  }

  return (
    <li
      data-testid={`watch-row-${expr}`}
      className="flex items-center gap-2 border-b border-(--color-border) px-2 py-1"
    >
      <code
        className="max-w-[40%] truncate font-mono text-(--color-syn-keyword)"
        title={expr}
      >
        {expr}
      </code>
      <span className="shrink-0 text-(--color-fg-tertiary)">·</span>
      <span className="flex-1 truncate font-mono text-sm">{renderValue()}</span>
      <button
        type="button"
        title="Remove watch"
        aria-label={`Remove watch for ${expr}`}
        data-testid={`watch-remove-${expr}`}
        onClick={() => remove(expr)}
        className="flex h-5 w-5 items-center justify-center text-(--color-fg-tertiary) hover:text-(--color-danger)"
      >
        <X size={12} aria-hidden />
      </button>
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
    <div data-testid="watch-view" className="flex h-full flex-col">
      {watches.length === 0 ? (
        <p
          data-testid="watch-empty"
          className="px-1 py-2 text-xs text-(--color-fg-tertiary)"
        >
          No watch expressions. Add a Solidity expression below — it&apos;s
          re-evaluated on every snapshot change.
        </p>
      ) : (
        <ul className="flex-1 overflow-auto">
          {watches.map((expr) => (
            <WatchRow key={expr} expr={expr} snapshotId={snapshotId} />
          ))}
        </ul>
      )}
      <form
        onSubmit={submit}
        className="mt-2 flex items-center gap-2 border-t border-(--color-border) bg-(--color-bg) px-2 py-2"
      >
        <Plus size={12} className="text-(--color-fg-tertiary)" aria-hidden />
        <input
          data-testid="watch-input"
          aria-label="Add watch expression"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          placeholder="e.g. balanceOf(msg.sender)"
          className="flex-1 bg-transparent font-mono text-sm outline-none placeholder:text-(--color-fg-tertiary)"
        />
        <button
          type="submit"
          data-testid="watch-add"
          disabled={draft.trim().length === 0}
          className="rounded border border-(--color-border) bg-(--color-bg-elevated) px-2 py-0.5 text-xs text-(--color-fg-secondary) hover:bg-(--color-bg-hover) hover:text-(--color-fg) disabled:opacity-40"
        >
          Add
        </button>
      </form>
    </div>
  );
}
