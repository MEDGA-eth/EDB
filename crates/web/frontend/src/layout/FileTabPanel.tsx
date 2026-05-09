import { openSearchPanel } from '@codemirror/search';
import { Bug, Code2, Copy, FileCode2, Hash, Search, WrapText } from 'lucide-react';
import { useCodeByAddress } from '../hooks/useCodeByAddress';
import { useSolidityEditor } from '../hooks/useSolidityEditor';
import { ErrorBoundary } from '../components/ErrorBoundary';
import { ErrorCard } from '../components/ErrorCard';
import { tokenize } from '../lib/opcodeTokens';
import { Toolbar, ToolbarButton, ToolbarDivider } from '../components/Toolbar';
import { useSession } from '../store/session';

/** marker path used for the synthetic disassembly tab when an address has no Solidity */
export const DISASM_PATH = '<disasm>';

/** Dockview passes parameters as `params` on the panel-props object. */
export interface FileTabPanelParams {
  addr: string;
  path: string;
}

export function FileTabPanel(
  props: { params?: FileTabPanelParams } | { addr: string; path: string },
) {
  const params: FileTabPanelParams =
    'params' in props && props.params
      ? props.params
      : (props as { addr: string; path: string });
  return (
    <ErrorBoundary label="FileTabPanel">
      <FileTabPanelInner addr={params.addr} path={params.path} />
    </ErrorBoundary>
  );
}

function FileTabPanelInner({ addr, path }: { addr: string; path: string }) {
  const { data, isLoading, error, refetch } = useCodeByAddress(addr);

  if (isLoading)
    return (
      <div className="p-4 text-(--color-fg-tertiary)" data-testid="file-tab-loading">
        Loading…
      </div>
    );
  if (error) return <ErrorCard message={(error as Error).message} onRetry={() => refetch()} />;
  if (!data) return null;

  if (data.kind === 'Opcodes') return <OpcodesView addr={addr} disasm={data.disasm} />;
  return <SolidityView addr={addr} path={path} files={data.files} entry={data.entry} hasOpcodes={false} />;
}

function OpcodesView({ addr, disasm }: { addr: string; disasm: string }) {
  return (
    <div className="flex h-full flex-col">
      <Toolbar testid="file-toolbar-opcodes">
        <ToolbarButton
          icon={Copy}
          label="Copy disassembly"
          testid="file-copy"
          onClick={() => {
            if (navigator.clipboard?.writeText) void navigator.clipboard.writeText(disasm);
          }}
        />
        <ToolbarDivider />
        <span
          className="font-display text-[11px] text-(--color-fg-tertiary)"
          data-testid="file-toolbar-meta"
        >
          {addr.slice(0, 10)}… · disasm
        </span>
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
  addr,
  path,
  files,
  entry,
}: {
  addr: string;
  path: string;
  files: { path: string; content: string }[];
  entry: string;
  hasOpcodes: boolean;
}) {
  const file =
    files.find((f) => f.path === path) ?? files.find((f) => f.path === entry) ?? files[0];
  const wordWrap = useSession((s) => s.wordWrap);
  const showLineNumbers = useSession((s) => s.showLineNumbers);
  const addBreakpoint = useSession((s) => s.addBreakpoint);
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
  function addBreakpointAtCursor() {
    const view = viewRef.current;
    if (!view || !file) return;
    const head = view.state.selection.main.head;
    const line = view.state.doc.lineAt(head).number;
    addBreakpoint({
      loc: { kind: 'Source', bytecode_address: addr, file_path: file.path, line_number: line },
      condition: null,
    });
  }

  return (
    <div className="flex h-full flex-col">
      <Toolbar testid="file-toolbar-source">
        <ToolbarButton
          icon={Search}
          label="Find in file (Ctrl+F)"
          testid="file-find"
          onClick={findInFile}
        />
        <ToolbarButton
          icon={Copy}
          label="Copy file content"
          testid="file-copy"
          onClick={copyContent}
        />
        <ToolbarDivider />
        <ToolbarButton
          icon={WrapText}
          label="Toggle word wrap"
          testid="file-wrap"
          active={wordWrap}
          onClick={() => useSession.getState().toggleWordWrap()}
        />
        <ToolbarButton
          icon={Hash}
          label="Toggle line numbers"
          testid="file-line-numbers"
          active={showLineNumbers}
          onClick={() => useSession.getState().toggleLineNumbers()}
        />
        <ToolbarDivider />
        <ToolbarButton
          icon={Bug}
          label="Add breakpoint at cursor line"
          testid="file-add-breakpoint"
          onClick={addBreakpointAtCursor}
        />
        <ToolbarDivider />
        <span
          className="ml-1 inline-flex items-center gap-1 font-display text-[11px] text-(--color-fg-tertiary)"
          data-testid="file-toolbar-meta"
        >
          <FileCode2 size={11} />
          {addr.slice(0, 10)}… · {file?.path ?? ''}
          <Code2 size={11} />
        </span>
      </Toolbar>
      <div ref={containerRef} data-testid="solidity-view" className="flex-1 overflow-auto" />
    </div>
  );
}
