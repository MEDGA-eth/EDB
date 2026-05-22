import { useEffect, useMemo, useRef, useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import ReactMarkdown from 'react-markdown';
import { ArrowDownToLine, Copy, Eye, Trash2, X } from 'lucide-react';
import { useEvalExpr } from '../../hooks/useEvalExpr';
import { useSnapshotCount } from '../../hooks/useSnapshotCount';
import { highlightSolidity } from '../../lib/solHighlight';
import { runTermCommand } from '../../lib/termCommands';
import { formatSolValue, type EvalResult } from '../../lib/types';
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
    const detail = e instanceof Error ? `, ${e.message}` : '';
    return `[unserializable: ${typeof value}]${detail}`;
  }
}

/**
 * Render an `EvalResult` as a single line for terminal display.
 *
 * We avoid `prettyJson` for the success case because the SolValue envelope
 * (`{ type, value: { bits, value } }`) is verbose and noisy in the
 * common case of a single-uint-256 or single-bool result. `formatSolValue`
 * collapses each variant to a short, human-readable form. We still fall
 * back to `prettyJson` for unexpected envelopes.
 */
export function formatEvalResult(value: unknown): string {
  if (value && typeof value === 'object' && 'kind' in value) {
    const r = value as EvalResult;
    if (r.kind === 'Ok') return formatSolValue(r.value);
    if (r.kind === 'Err') return `error: ${r.error}`;
  }
  return prettyJson(value);
}

/**
 * First-token completions offered in the terminal: built-in command verbs
 * plus the `$edb_*` expression builtins. Trailing space / `(` positions the
 * caret for arguments once accepted. Single-letter aliases (s/n/c/…) are
 * intentionally omitted to keep the list focused.
 */
const TERM_COMPLETIONS = [
  'step',
  'next',
  'over',
  'out',
  'continue',
  'reverse-continue',
  'goto ',
  'break ',
  'bp',
  'unbreak ',
  'clear',
  'help',
  'edb_sload(',
  'edb_tsload(',
  'edb_stack(',
  'edb_memory(',
  'edb_calldata(',
  'keccak256(',
  'edb_help()',
];

/**
 * Completions for the current input: only when typing the first token (no
 * whitespace yet) and the token is a non-empty prefix of one or more
 * candidates, excluding an exact match so the list disappears once a verb is
 * fully typed. Capped to keep the dropdown compact.
 */
export function computeTermSuggestions(input: string): string[] {
  const tok = input.trimStart();
  if (tok.length === 0 || /\s/.test(tok)) return [];
  const lc = tok.toLowerCase();
  return TERM_COMPLETIONS.filter(
    (c) => c.toLowerCase().startsWith(lc) && c.trimEnd() !== tok,
  ).slice(0, 8);
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
  const addWatch = useSession((s) => s.addWatchExpression);
  const evalExpr = useEvalExpr();
  const qc = useQueryClient();
  const snapshotCountQ = useSnapshotCount();
  const [input, setInput] = useState('');
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const inputElRef = useRef<HTMLInputElement | null>(null);
  // Command-history recall (Up/Down). `histCursorRef` indexes into the
  // chronological list of prior input texts; null means "at the live draft".
  // `draftRef` stashes whatever the user was typing before they started
  // browsing history so Down past the newest restores it.
  const histCursorRef = useRef<number | null>(null);
  const draftRef = useRef('');
  // Autocomplete dropdown state. Suggestions derive from `input`; `dismissed`
  // (cleared on the next keystroke) lets Escape hide the list without
  // clearing the input, and `focused` hides it when the field loses focus.
  const [suggestActive, setSuggestActive] = useState(0);
  const [suggestDismissed, setSuggestDismissed] = useState(false);
  const [focused, setFocused] = useState(false);
  // Monotonically-increasing submission counter, used to pair an input
  // entry with its result/error so concurrent evals render in input-order
  // even when the server replies out-of-order.
  const submissionCounterRef = useRef(0);
  // Active AbortControllers, keyed by submissionId, so the user can cancel
  // a specific in-flight eval. We only display one Cancel button at a time
  // (the most recent submission), which is the common case.
  const controllersRef = useRef<Map<number, AbortController>>(new Map());
  // Re-render trigger when the in-flight set changes, refs alone don't
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
    if (last.kind === 'result') text = `${last.expr} → ${formatEvalResult(last.value)}`;
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

  function lastInputText(): string | null {
    for (let i = history.length - 1; i >= 0; i -= 1) {
      const h = history[i];
      if (h.kind === 'input') return h.text;
    }
    return null;
  }

  // Chronological list of prior submitted inputs, for Up/Down recall.
  const inputTexts = useMemo(
    () => history.filter((h) => h.kind === 'input').map((h) => h.text),
    [history],
  );

  const suggestions = useMemo(() => computeTermSuggestions(input), [input]);
  const showSuggest = focused && !suggestDismissed && suggestions.length > 0;

  // Keep the active suggestion in range as the list changes.
  useEffect(() => {
    setSuggestActive((a) => (a >= suggestions.length ? 0 : a));
  }, [suggestions.length]);

  function recallPrev() {
    if (inputTexts.length === 0) return;
    if (histCursorRef.current === null) {
      draftRef.current = input;
      histCursorRef.current = inputTexts.length - 1;
    } else {
      histCursorRef.current = Math.max(0, histCursorRef.current - 1);
    }
    setInput(inputTexts[histCursorRef.current]!);
  }

  function recallNext() {
    if (histCursorRef.current === null) return;
    const idx = histCursorRef.current + 1;
    if (idx >= inputTexts.length) {
      histCursorRef.current = null;
      setInput(draftRef.current);
    } else {
      histCursorRef.current = idx;
      setInput(inputTexts[idx]!);
    }
  }

  function acceptSuggestion(s: string) {
    setInput(s);
    setSuggestDismissed(true);
    histCursorRef.current = null;
    inputElRef.current?.focus();
  }

  function onInputKeyDown(e: React.KeyboardEvent<HTMLInputElement>) {
    // Tab accepts the highlighted completion (when the dropdown is open).
    if (e.key === 'Tab' && showSuggest) {
      e.preventDefault();
      acceptSuggestion(suggestions[suggestActive] ?? suggestions[0]!);
      return;
    }
    if (e.key === 'Escape' && showSuggest) {
      e.preventDefault();
      setSuggestDismissed(true);
      return;
    }
    // Arrows are reserved for command history so a previous command is always
    // one keypress away (the dropdown is driven by Tab / click instead).
    if (e.key === 'ArrowUp') {
      e.preventDefault();
      recallPrev();
      return;
    }
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      recallNext();
    }
  }

  function saveLastAsWatch() {
    const last = lastInputText();
    if (last) addWatch(last);
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
    // An empty submission re-runs the most recent command (shell convention).
    let expr = input;
    if (!expr.trim()) {
      const last = lastInputText();
      if (!last) return;
      expr = last;
    }
    const ts = Date.now();
    submissionCounterRef.current += 1;
    const submissionId = submissionCounterRef.current;
    append({ kind: 'input', ts, text: expr, submissionId });
    setInput('');
    histCursorRef.current = null;
    setSuggestDismissed(false);

    // First try built-in commands (step / goto / bp / …). They run
    // synchronously and return either a markdown message or nothing.
    const cmdResult = runTermCommand(expr, {
      queryClient: qc,
      snapshotCount: snapshotCountQ.data ?? 0,
    });
    if (cmdResult.handled) {
      if (cmdResult.message) {
        append({ kind: 'message', ts: Date.now(), text: cmdResult.message });
      }
      return;
    }

    const controller = new AbortController();
    controllersRef.current.set(submissionId, controller);
    setPendingCount(controllersRef.current.size);
    try {
      const value = await evalExpr.mutateAsync({ id, expr, signal: controller.signal });
      append({ kind: 'result', ts: Date.now(), expr, value, submissionId });
    } catch (e) {
      // AbortError gets a friendlier "Cancelled" surface, not a normal error.
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
          label="Clear"
          showLabel
          testid="terminal-clear"
          onClick={clear}
          disabled={history.length === 0}
        />
        <ToolbarButton
          icon={Copy}
          label="Copy last"
          showLabel
          testid="terminal-copy-last"
          onClick={() => void copyLast()}
          disabled={history.length === 0}
        />
        <ToolbarButton
          icon={Eye}
          label="Save as watch"
          showLabel
          testid="terminal-save-watch"
          onClick={saveLastAsWatch}
          disabled={lastInputText() === null}
        />
        <ToolbarDivider />
        <ToolbarButton
          icon={ArrowDownToLine}
          label="Scroll to bottom"
          showLabel
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
      <form onSubmit={submit} className="relative flex items-center gap-2 border-t border-(--color-border) bg-(--color-bg) p-2">
        {showSuggest && (
          <ul
            role="listbox"
            data-testid="terminal-suggestions"
            className="absolute bottom-full left-2 right-2 z-20 mb-1 max-h-56 overflow-auto rounded border border-(--color-border) bg-(--color-bg-elevated) py-1 shadow-[var(--shadow-md)]"
          >
            {suggestions.map((s, i) => (
              <li key={s}>
                <button
                  type="button"
                  role="option"
                  aria-selected={i === suggestActive}
                  data-testid="terminal-suggestion"
                  // Keep the input focused so onClick fires before blur closes us.
                  onMouseDown={(e) => e.preventDefault()}
                  onMouseEnter={() => setSuggestActive(i)}
                  onClick={() => acceptSuggestion(s)}
                  className={`flex w-full items-center px-3 py-1 text-left font-mono text-xs ${
                    i === suggestActive
                      ? 'bg-(--color-bg-active) text-(--color-fg)'
                      : 'text-(--color-fg-secondary) hover:bg-(--color-bg-hover)'
                  }`}
                >
                  {s}
                </button>
              </li>
            ))}
            <li className="px-3 pt-1 text-[10px] text-(--color-fg-tertiary)">
              Tab to complete · ↑/↓ history
            </li>
          </ul>
        )}
        <span className="text-(--color-accent) font-bold">›</span>
        <input
          ref={inputElRef}
          data-testid="terminal-input"
          value={input}
          onChange={(e) => {
            setInput(e.target.value);
            histCursorRef.current = null;
            setSuggestDismissed(false);
          }}
          onKeyDown={onInputKeyDown}
          onFocus={() => setFocused(true)}
          onBlur={() => setFocused(false)}
          autoComplete="off"
          spellCheck={false}
          className="flex-1 bg-transparent font-mono outline-none"
          placeholder="Solidity expression or command (type `help`)"
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
  if (entry.kind === 'input')
    return (
      <div data-testid="term-input">
        <span className="text-(--color-accent)">›</span> {highlightSolidity(entry.text)}
      </div>
    );
  if (entry.kind === 'error')
    return (
      <div data-testid="term-error" className="text-(--color-danger)">
        ⨯ {entry.message} ({entry.code})
      </div>
    );
  if (entry.kind === 'message')
    return (
      <div data-testid="term-message" className="text-(--color-fg-secondary)">
        <ReactMarkdown>{entry.text}</ReactMarkdown>
      </div>
    );
  return (
    <div data-testid="term-result">
      <div className="opacity-70">{highlightSolidity(entry.expr)}{' →'}</div>
      <pre className="my-1 whitespace-pre-wrap rounded bg-(--color-bg-elevated) px-2 py-1 text-(--color-fg)">
        {formatEvalResult(entry.value)}
      </pre>
    </div>
  );
}
