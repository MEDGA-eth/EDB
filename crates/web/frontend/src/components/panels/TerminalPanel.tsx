import { useState } from 'react';
import ReactMarkdown from 'react-markdown';
import { useEvalExpr } from '../../hooks/useEvalExpr';
import { useSession } from '../../store/session';
import { ErrorBoundary } from '../ErrorBoundary';

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
  const evalExpr = useEvalExpr();
  const [input, setInput] = useState('');

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
      append({ kind: 'error', ts: Date.now(), expr, code: err.code ?? -1, message: err.message ?? String(e) });
    }
  }

  return (
    <div className="flex h-full flex-col" data-testid="terminal-panel">
      <div className="flex-1 overflow-auto p-3 font-mono text-sm">
        {history.map((h, i) => <TerminalLine key={i} entry={h} />)}
      </div>
      <form onSubmit={submit} className="flex gap-2 border-t border-(--color-border) bg-(--color-bg) p-2">
        <span className="text-(--color-accent) font-bold">›</span>
        <input data-testid="terminal-input" value={input}
               onChange={(e) => setInput(e.target.value)}
               className="flex-1 bg-transparent font-mono outline-none" placeholder="Solidity expression…" />
      </form>
    </div>
  );
}

function TerminalLine({ entry }: { entry: import('../../store/session').TerminalEntry }) {
  if (entry.kind === 'input') return <div data-testid="term-input">› {entry.text}</div>;
  if (entry.kind === 'error')
    return <div data-testid="term-error" className="text-(--color-danger)">⨯ {entry.message} ({entry.code})</div>;
  return (
    <div data-testid="term-result">
      <ReactMarkdown>{`\`${entry.expr}\` →\n\n\`\`\`\n${JSON.stringify(entry.value, null, 2)}\n\`\`\``}</ReactMarkdown>
    </div>
  );
}
