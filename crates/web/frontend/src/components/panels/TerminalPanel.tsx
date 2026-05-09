import { useEffect, useRef, useState } from 'react';
import ReactMarkdown from 'react-markdown';
import { ArrowDownToLine, Copy, Trash2, X } from 'lucide-react';
import { useEvalExpr } from '../../hooks/useEvalExpr';
import { useSession } from '../../store/session';
import { ErrorBoundary } from '../ErrorBoundary';
import { Toolbar, ToolbarButton, ToolbarDivider } from '../Toolbar';

/**
 * Pretty-print an RPC eval result without losing numeric precision.
 *
 * The engine encodes uint256 / int256 / hash values as decimal or hex
 * strings (so a JS number can never silently truncate them). However, in
 * case a future build accidentally returns a `bigint` literal in JSON,
 * we add a replacer that stringifies it so `JSON.stringify` doesn't throw.
 *
 * `JSON.stringify` can still throw for circular references, BigInts that
 * escape the replacer (older engines), or odd host objects. Catch those
 * so the terminal renders a typed fallback instead of crashing the panel.
 */
export function prettyJson(value: unknown): string {
  try {
    return JSON.stringify(
      value,
      (_k, v) => (typeof v === 'bigint' ? v.toString() : v),
      2,
    );
  } catch (e) {
    const detail = e instanceof Error ? ` — ${e.message}` : '';
    return `[unserializable: ${typeof value}]${detail}`;
  }
}

export function TerminalPanel() {
  return (
    <ErrorBoundary label="TerminalPanel">
      <TerminalPanelInner />
    </ErrorBoundary>
  );
}

function TerminalPanelInner() {
  const id = useSession((s) => s.currentSnapshotId);
  const history = useSession((s) => s.terminalHistory);
  const append = useSession((s) => s.appendTerminal);
  const clear = useSession((s) => s.clearTerminal);
  const evalExpr = useEvalExpr();
  const [input, setInput] = useState('');
  const scrollRef = useRef<HTMLDivElement | null>(null);
  // Monotonically-increasing submission counter — used to pair an input
  // entry with its result/error so concurrent evals render in input-order
  // even when the server replies out-of-order.
  const submissionCounterRef = useRef(0);
  // Active AbortControllers, keyed by submissionId, so the user can cancel
  // a specific in-flight eval. We only display one Cancel button at a time
  // (the most recent submission), which is the common case.
  const controllersRef = useRef<Map<number, AbortController>>(new Map());
  // Re-render trigger when the in-flight set changes — refs alone don't
  // notify React so we mirror "is anything pending" into state.
  const [pendingCount, setPendingCount] = useState(0);

  function scrollToBottom() {
    if (scrollRef.current) scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
  }

  // auto-scroll on new entries
  useEffect(() => {
    scrollToBottom();
  }, [history.length]);

  // Abort everything still in-flight on unmount so we don't dangle fetches.
  useEffect(() => {
    const controllers = controllersRef.current;
    return () => {
      for (const c of controllers.values()) {
        try { c.abort(); } catch { /* ignore */ }
      }
      controllers.clear();
    };
  }, []);

  async function copyLast() {
    const last = [...history].reverse().find((h) => h.kind === 'result' || h.kind === 'error');
    if (!last) return;
    let text = '';
    if (last.kind === 'result') text = `${last.expr} → ${prettyJson(last.value)}`;
    else if (last.kind === 'error') text = `${last.expr} ⨯ ${last.message} (${last.code})`;
    if (!navigator.clipboard?.writeText) {
      append({
        kind: 'error',
        ts: Date.now(),
        expr: '(copy)',
        code: -1,
        message: 'Clipboard API not available in this context.',
      });
      return;
    }
    try {
      await navigator.clipboard.writeText(text);
    } catch (e) {
      const err = e as { message?: string };
      append({
        kind: 'error',
        ts: Date.now(),
        expr: '(copy)',
        code: -1,
        message: `Clipboard write failed: ${err.message ?? String(e)}`,
      });
    }
  }

  function cancelLatest() {
    const map = controllersRef.current;
    if (map.size === 0) return;
    // Highest submissionId is the most recent.
    let latestId = -1;
    for (const k of map.keys()) if (k > latestId) latestId = k;
    const c = map.get(latestId);
    if (c) c.abort();
  }

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (!input.trim()) return;
    const ts = Date.now();
    submissionCounterRef.current += 1;
    const submissionId = submissionCounterRef.current;
    append({ kind: 'input', ts, text: input, submissionId });
    const expr = input;
    setInput('');
    const controller = new AbortController();
    controllersRef.current.set(submissionId, controller);
    setPendingCount(controllersRef.current.size);
    try {
      const value = await evalExpr.mutateAsync({ id, expr, signal: controller.signal });
      append({ kind: 'result', ts: Date.now(), expr, value, submissionId });
    } catch (e) {
      // AbortError gets a friendlier "Cancelled" surface — not a normal error.
      const isAbort =
        (typeof DOMException !== 'undefined' && e instanceof DOMException && e.name === 'AbortError') ||
        (e instanceof Error && e.name === 'AbortError');
      if (isAbort) {
        append({
          kind: 'error',
          ts: Date.now(),
          expr,
          code: 0,
          message: 'Cancelled',
          submissionId,
        });
      } else {
        const err = e as { code?: number; message?: string };
        append({
          kind: 'error',
          ts: Date.now(),
          expr,
          code: err.code ?? -1,
          message: err.message ?? String(e),
          submissionId,
        });
      }
    } finally {
      controllersRef.current.delete(submissionId);
      setPendingCount(controllersRef.current.size);
    }
  }

  return (
    <div className="flex h-full flex-col" data-testid="terminal-panel">
      <Toolbar testid="terminal-toolbar">
        <ToolbarButton
          icon={Trash2}
          label="Clear history"
          testid="terminal-clear"
          onClick={clear}
          disabled={history.length === 0}
        />
        <ToolbarButton
          icon={Copy}
          label="Copy last result"
          testid="terminal-copy-last"
          onClick={() => void copyLast()}
          disabled={history.length === 0}
        />
        <ToolbarDivider />
        <ToolbarButton
          icon={ArrowDownToLine}
          label="Scroll to bottom"
          testid="terminal-scroll-bottom"
          onClick={scrollToBottom}
        />
      </Toolbar>
      <div ref={scrollRef} className="flex-1 overflow-auto p-3 font-mono text-sm">
        {history.map((h, i) => (
          // `ts` collisions are vanishingly rare in practice; fall back to the
          // index when two entries share the same millisecond.
          <TerminalLine key={`${h.ts}-${i}`} entry={h} />
        ))}
      </div>
      <form onSubmit={submit} className="flex items-center gap-2 border-t border-(--color-border) bg-(--color-bg) p-2">
        <span className="text-(--color-accent) font-bold">›</span>
        <input
          data-testid="terminal-input"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          className="flex-1 bg-transparent font-mono outline-none"
          placeholder="Solidity expression…"
        />
        {pendingCount > 0 && (
          <button
            type="button"
            data-testid="terminal-cancel"
            onClick={cancelLatest}
            aria-label="Cancel evaluation"
            title="Cancel evaluation"
            className="inline-flex items-center gap-1 rounded border border-(--color-border) bg-(--color-bg-elevated) px-2 py-0.5 text-xs text-(--color-fg-secondary) hover:bg-(--color-bg-hover) hover:text-(--color-fg)"
          >
            <X size={10} aria-hidden />
            Cancel
          </button>
        )}
      </form>
    </div>
  );
}

function TerminalLine({ entry }: { entry: import('../../store/session').TerminalEntry }) {
  if (entry.kind === 'input') return <div data-testid="term-input">› {entry.text}</div>;
  if (entry.kind === 'error')
    return (
      <div data-testid="term-error" className="text-(--color-danger)">
        ⨯ {entry.message} ({entry.code})
      </div>
    );
  return (
    <div data-testid="term-result">
      <ReactMarkdown>{`\`${entry.expr}\` →\n\n\`\`\`\n${prettyJson(entry.value)}\n\`\`\``}</ReactMarkdown>
    </div>
  );
}
