import { HighlightStyle, syntaxHighlighting } from '@codemirror/language';
import { search, openSearchPanel } from '@codemirror/search';
import { Compartment, EditorState } from '@codemirror/state';
import { EditorView, lineNumbers } from '@codemirror/view';
import { tags as t } from '@lezer/highlight';
import { solidity } from '@replit/codemirror-lang-solidity';
import { useEffect, useRef } from 'react';
import { Bug, Code2, Copy, FileCode2, Hash, Search, WrapText } from 'lucide-react';
import { useCodeByAddress } from '../hooks/useCodeByAddress';
import { ErrorBoundary } from '../components/ErrorBoundary';
import { ErrorCard } from '../components/ErrorCard';
import { tokenize } from '../lib/opcodeTokens';
import { Toolbar, ToolbarButton, ToolbarDivider } from '../components/Toolbar';
import { useSession } from '../store/session';

/** marker path used for the synthetic disassembly tab when an address has no Solidity */
export const DISASM_PATH = '<disasm>';

const edbHighlight = HighlightStyle.define([
  { tag: t.keyword, color: 'var(--color-syn-keyword)', fontWeight: '600' },
  { tag: [t.string, t.special(t.string)], color: 'var(--color-syn-string)' },
  { tag: [t.number, t.bool, t.null], color: 'var(--color-syn-number)' },
  { tag: t.comment, color: 'var(--color-syn-comment)', fontStyle: 'italic' },
  { tag: [t.typeName, t.className, t.namespace], color: 'var(--color-syn-type)' },
  {
    tag: [t.function(t.variableName), t.function(t.propertyName)],
    color: 'var(--color-syn-func)',
  },
  { tag: [t.operator, t.operatorKeyword], color: 'var(--color-fg)' },
  { tag: t.variableName, color: 'var(--color-fg)' },
  { tag: t.propertyName, color: 'var(--color-fg-secondary)' },
  { tag: t.punctuation, color: 'var(--color-fg-tertiary)' },
]);

const edbTheme = EditorView.theme({
  '&': {
    fontFamily: 'var(--font-mono)',
    fontSize: '13px',
    backgroundColor: 'transparent',
  },
  '.cm-content': { caretColor: 'var(--color-fg)' },
  '.cm-gutters': {
    backgroundColor: 'transparent',
    color: 'var(--color-fg-tertiary)',
    border: 'none',
  },
  '.cm-activeLine': { backgroundColor: 'var(--color-bg-hover)' },
  '.cm-activeLineGutter': { backgroundColor: 'var(--color-bg-hover)' },
  '.cm-selectionBackground, & ::selection': {
    backgroundColor: 'var(--color-accent-dim)',
  },
});

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
  const ref = useRef<HTMLDivElement | null>(null);
  const viewRef = useRef<EditorView | null>(null);
  const wrapCmpRef = useRef<Compartment>(new Compartment());
  const lnCmpRef = useRef<Compartment>(new Compartment());
  const wordWrap = useSession((s) => s.wordWrap);
  const showLineNumbers = useSession((s) => s.showLineNumbers);
  const addBreakpoint = useSession((s) => s.addBreakpoint);

  useEffect(() => {
    if (!ref.current || !file) return;
    const wrapCmp = wrapCmpRef.current;
    const lnCmp = lnCmpRef.current;
    const state = EditorState.create({
      doc: file.content,
      extensions: [
        lnCmp.of(showLineNumbers ? [lineNumbers()] : []),
        solidity,
        syntaxHighlighting(edbHighlight),
        edbTheme,
        search(),
        wrapCmp.of(wordWrap ? [EditorView.lineWrapping] : []),
        EditorView.editable.of(false),
      ],
    });
    const view = new EditorView({ state, parent: ref.current });
    viewRef.current = view;
    return () => {
      view.destroy();
      viewRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [file]);

  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    view.dispatch({
      effects: wrapCmpRef.current.reconfigure(wordWrap ? [EditorView.lineWrapping] : []),
    });
  }, [wordWrap]);
  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    view.dispatch({
      effects: lnCmpRef.current.reconfigure(showLineNumbers ? [lineNumbers()] : []),
    });
  }, [showLineNumbers]);

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
      <div ref={ref} data-testid="solidity-view" className="flex-1 overflow-auto" />
    </div>
  );
}
