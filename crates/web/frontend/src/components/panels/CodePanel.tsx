import { openSearchPanel } from '@codemirror/search';
import { Copy, Hash, Locate, Search, WrapText } from 'lucide-react';
import { useEffect, useRef } from 'react';
import { useCode } from '../../hooks/useCode';
import { useSnapshotInfo } from '../../hooks/useSnapshotInfo';
import { useSession } from '../../store/session';
import { useSolidityEditor } from '../../hooks/useSolidityEditor';
import { ErrorBoundary } from '../ErrorBoundary';
import { ErrorCard } from '../ErrorCard';
import { pcLineIndex, tokenize } from '../../lib/opcodeTokens';
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
  if (error)
    return (
      <ErrorCard
        message={(error as Error).message}
        cause={error}
        onRetry={() => refetch()}
      />
    );
  if (!data) return null;

  if (data.kind === 'Opcodes')
    return <OpcodesView disasm={data.disasm} bytecodeAddress={data.bytecode_address} />;
  return <SolidityView entry={data.entry} files={data.files} />;
}

function OpcodesView({
  disasm,
  bytecodeAddress,
}: {
  disasm: string;
  bytecodeAddress: string;
}) {
  // Highlight the disasm line whose PC matches the active opcode snapshot,
  // but only when that snapshot belongs to *this* contract — guards against
  // the panel briefly showing a stale highlight while user navigates between
  // frames at different addresses.
  const id = useSession((s) => s.currentSnapshotId);
  const { data: snap } = useSnapshotInfo(id);
  const currentLine = (() => {
    if (!snap || snap.detail.kind !== 'Opcode') return -1;
    if (snap.bytecode_address.toLowerCase() !== bytecodeAddress.toLowerCase()) return -1;
    return pcLineIndex(disasm, snap.detail.pc);
  })();
  // See FileTabPanel.tsx for why we use data-attribute querying instead
  // of a single swappable ref: the single-ref pattern dropped to null on
  // backward navigation (Reverse Step) because React processed the newly
  // current div first and then the previously-current div, the latter
  // clearing the shared ref.
  const preRef = useRef<HTMLPreElement | null>(null);
  function scrollCurrentIntoView() {
    const el = preRef.current?.querySelector<HTMLDivElement>(
      '[data-edb-current="true"]',
    );
    el?.scrollIntoView({ block: 'center', behavior: 'auto' });
  }
  useEffect(() => {
    if (currentLine < 0) return;
    scrollCurrentIntoView();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [currentLine]);
  function revealCurrent() {
    scrollCurrentIntoView();
  }
  // Pulse from the global "Where am I?" button.
  const revealTick = useSession((s) => s.revealTick);
  useEffect(() => {
    if (revealTick === 0 || currentLine < 0) return;
    scrollCurrentIntoView();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [revealTick]);
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
        <ToolbarButton
          icon={Locate}
          label="Reveal current PC"
          testid="code-reveal-current"
          onClick={revealCurrent}
          disabled={currentLine < 0}
        />
      </Toolbar>
      <pre
        ref={preRef}
        data-testid="opcodes-view"
        className="flex-1 overflow-auto p-4 font-mono text-sm leading-relaxed"
      >
        {disasm.split('\n').map((line, i) => {
          const isCurrent = i === currentLine;
          return (
            <div
              key={i}
              data-edb-current={isCurrent ? 'true' : undefined}
              className={isCurrent ? 'opcodes-current-line -mx-4 px-4' : undefined}
            >
              {tokenize(line).map((tok, j) => (
                <span key={j} className={`syn-${tok.kind}`}>
                  {tok.text}
                </span>
              ))}
            </div>
          );
        })}
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
