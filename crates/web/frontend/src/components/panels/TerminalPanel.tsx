import { useEffect, useRef, useState } from 'react';
import ReactMarkdown from 'react-markdown';
import { ArrowDownToLine, Copy, Trash2 } from 'lucide-react';
import { useEvalExpr } from '../../hooks/useEvalExpr';
import { useSession } from '../../store/session';
import { ErrorBoundary } from '../ErrorBoundary';
import { Toolbar, ToolbarButton, ToolbarDivider } from '../Toolbar';

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

  function scrollToBottom() {
    if (scrollRef.current) scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
  }

  // auto-scroll on new entries
  useEffect(() => {
    scrollToBottom();
  }, [history.length]);

  async function copyLast() {
    const last = [...history].reverse().find((h) => h.kind === 'result' || h.kind === 'error');
    if (!last) return;
    let text = '';
    if (last.kind === 'result') text = `${last.expr} → ${JSON.stringify(last.value, null, 2)}`;
    else if (last.kind === 'error') text = `${last.expr} ⨯ ${last.message} (${last.code})`;
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
    }
  }

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (!input.trim()) return;
    const ts = Date.now();
    append({ kind: 'input', ts, text: input });
    const expr = input;
    setInput('');
    try {
      const value = await evalExpr.mutateAsync({ id, expr });
      append({ kind: 'result', ts: Date.now(), expr, value });
    } catch (e) {
      const err = e as { code?: number; message?: string };
      append({
        kind: 'error',
        ts: Date.now(),
        expr,
        code: err.code ?? -1,
        message: err.message ?? String(e),
      });
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
          <TerminalLine key={i} entry={h} />
        ))}
      </div>
      <form onSubmit={submit} className="flex gap-2 border-t border-(--color-border) bg-(--color-bg) p-2">
        <span className="text-(--color-accent) font-bold">›</span>
        <input
          data-testid="terminal-input"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          className="flex-1 bg-transparent font-mono outline-none"
          placeholder="Solidity expression…"
        />
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
      <ReactMarkdown>{`\`${entry.expr}\` →\n\n\`\`\`\n${JSON.stringify(entry.value, null, 2)}\n\`\`\``}</ReactMarkdown>
    </div>
  );
}
