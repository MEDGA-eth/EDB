import { HighlightStyle, syntaxHighlighting } from '@codemirror/language';
import { search, openSearchPanel } from '@codemirror/search';
import { Compartment, EditorState } from '@codemirror/state';
import { EditorView, lineNumbers } from '@codemirror/view';
import { tags as t } from '@lezer/highlight';
import { solidity } from '@replit/codemirror-lang-solidity';
import { useEffect, useRef } from 'react';
import { Copy, Hash, Search, WrapText } from 'lucide-react';
import { useCode } from '../../hooks/useCode';
import { useSession } from '../../store/session';
import { ErrorBoundary } from '../ErrorBoundary';
import { ErrorCard } from '../ErrorCard';
import { tokenize } from '../../lib/opcodeTokens';
import { Toolbar, ToolbarButton, ToolbarDivider } from '../Toolbar';

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
  const ref = useRef<HTMLDivElement | null>(null);
  const viewRef = useRef<EditorView | null>(null);
  const wrapCmpRef = useRef<Compartment>(new Compartment());
  const lnCmpRef = useRef<Compartment>(new Compartment());
  const wordWrap = useSession((s) => s.wordWrap);
  const showLineNumbers = useSession((s) => s.showLineNumbers);

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
    // re-initialise only when the file or compartment-fed config truly changes
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [file]);

  // re-configure compartments when toggles change
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
      <div ref={ref} data-testid="solidity-view" className="flex-1 overflow-auto" />
    </div>
  );
}
