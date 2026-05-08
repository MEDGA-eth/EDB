import { EditorState } from '@codemirror/state';
import { EditorView, lineNumbers } from '@codemirror/view';
import { solidity } from '@replit/codemirror-lang-solidity';
import { useEffect, useRef } from 'react';
import { useCode } from '../../hooks/useCode';
import { useSession } from '../../store/session';
import { ErrorBoundary } from '../ErrorBoundary';
import { ErrorCard } from '../ErrorCard';
import { tokenize } from '../../lib/opcodeTokens';

export function CodePanel() {
  return (
    <ErrorBoundary label="CodePanel">
      <CodePanelInner />
    </ErrorBoundary>
  );
}

function CodePanelInner() {
  const id = useSession((s) => s.currentSnapshotId);
  const { data, isLoading, error, refetch } = useCode(id);

  if (isLoading)
    return (
      <div className="p-4 text-(--color-fg-tertiary)" data-testid="code-loading">
        Loading…
      </div>
    );
  if (error) return <ErrorCard message={(error as Error).message} onRetry={() => refetch()} />;
  if (!data) return null;

  if (data.kind === 'Opcodes') return <OpcodesView disasm={data.disasm} />;
  return <SolidityView entry={data.entry} files={data.files} />;
}

function OpcodesView({ disasm }: { disasm: string }) {
  return (
    <pre
      data-testid="opcodes-view"
      className="h-full overflow-auto p-4 font-mono text-sm leading-relaxed"
    >
      {disasm.split('\n').map((line, i) => (
        <div key={i}>
          {tokenize(line).map((t, j) => (
            <span key={j} className={`syn-${t.kind}`}>
              {t.text}
            </span>
          ))}
        </div>
      ))}
    </pre>
  );
}

function SolidityView({
  entry,
  files,
}: {
  entry: string;
  files: { path: string; content: string }[];
}) {
  const file = files.find((f) => f.path === entry) ?? files[0];
  const ref = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!ref.current || !file) return;
    const state = EditorState.create({
      doc: file.content,
      extensions: [lineNumbers(), solidity, EditorView.editable.of(false)],
    });
    const view = new EditorView({ state, parent: ref.current });
    return () => view.destroy();
  }, [file]);

  return <div ref={ref} data-testid="solidity-view" className="h-full overflow-auto" />;
}
