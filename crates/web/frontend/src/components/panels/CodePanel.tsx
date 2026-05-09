import { openSearchPanel } from '@codemirror/search';
import { Copy, Hash, Search, WrapText } from 'lucide-react';
import { useCode } from '../../hooks/useCode';
import { useSession } from '../../store/session';
import { useSolidityEditor } from '../../hooks/useSolidityEditor';
import { ErrorBoundary } from '../ErrorBoundary';
import { ErrorCard } from '../ErrorCard';
import { tokenize } from '../../lib/opcodeTokens';
import { Toolbar, ToolbarButton, ToolbarDivider } from '../Toolbar';

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
    <div className="flex h-full flex-col">
      <Toolbar testid="code-toolbar-opcodes">
        <ToolbarButton
          icon={Copy}
          label="Copy disassembly"
          testid="code-copy"
          onClick={() => {
            if (navigator.clipboard?.writeText) void navigator.clipboard.writeText(disasm);
          }}
        />
      </Toolbar>
      <pre
        data-testid="opcodes-view"
        className="flex-1 overflow-auto p-4 font-mono text-sm leading-relaxed"
      >
        {disasm.split('\n').map((line, i) => (
          <div key={i}>
            {tokenize(line).map((tok, j) => (
              <span key={j} className={`syn-${tok.kind}`}>
                {tok.text}
              </span>
            ))}
          </div>
        ))}
      </pre>
    </div>
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
  const wordWrap = useSession((s) => s.wordWrap);
  const showLineNumbers = useSession((s) => s.showLineNumbers);
  const { containerRef, viewRef } = useSolidityEditor({
    content: file?.content ?? '',
    wordWrap,
    showLineNumbers,
  });

  function copyContent() {
    if (!file) return;
    if (navigator.clipboard?.writeText) void navigator.clipboard.writeText(file.content);
  }

  function findInFile() {
    const view = viewRef.current;
    if (view) openSearchPanel(view);
  }

  return (
    <div className="flex h-full flex-col">
      <Toolbar testid="code-toolbar-source">
        <ToolbarButton
          icon={Search}
          label="Find in file (Ctrl+F)"
          testid="code-find"
          onClick={findInFile}
        />
        <ToolbarButton
          icon={Copy}
          label="Copy file content"
          testid="code-copy"
          onClick={copyContent}
        />
        <ToolbarDivider />
        <ToolbarButton
          icon={WrapText}
          label="Toggle word wrap"
          testid="code-wrap"
          active={wordWrap}
          onClick={() => useSession.getState().toggleWordWrap()}
        />
        <ToolbarButton
          icon={Hash}
          label="Toggle line numbers"
          testid="code-line-numbers"
          active={showLineNumbers}
          onClick={() => useSession.getState().toggleLineNumbers()}
        />
      </Toolbar>
      <div ref={containerRef} data-testid="solidity-view" className="flex-1 overflow-auto" />
    </div>
  );
}
