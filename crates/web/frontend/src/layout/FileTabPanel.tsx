import { HighlightStyle, syntaxHighlighting } from '@codemirror/language';
import { EditorState } from '@codemirror/state';
import { EditorView, lineNumbers } from '@codemirror/view';
import { tags as t } from '@lezer/highlight';
import { solidity } from '@replit/codemirror-lang-solidity';
import { useEffect, useRef } from 'react';
import { useCodeByAddress } from '../hooks/useCodeByAddress';
import { ErrorBoundary } from '../components/ErrorBoundary';
import { ErrorCard } from '../components/ErrorCard';
import { tokenize } from '../lib/opcodeTokens';

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

  if (data.kind === 'Opcodes') return <OpcodesView disasm={data.disasm} />;
  return <SolidityView path={path} files={data.files} entry={data.entry} />;
}

function OpcodesView({ disasm }: { disasm: string }) {
  return (
    <pre
      data-testid="opcodes-view"
      className="h-full overflow-auto p-4 font-mono text-sm leading-relaxed"
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
  );
}

function SolidityView({
  path,
  files,
  entry,
}: {
  path: string;
  files: { path: string; content: string }[];
  entry: string;
}) {
  const file =
    files.find((f) => f.path === path) ?? files.find((f) => f.path === entry) ?? files[0];
  const ref = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!ref.current || !file) return;
    const state = EditorState.create({
      doc: file.content,
      extensions: [
        lineNumbers(),
        solidity,
        syntaxHighlighting(edbHighlight),
        edbTheme,
        EditorView.editable.of(false),
      ],
    });
    const view = new EditorView({ state, parent: ref.current });
    return () => view.destroy();
  }, [file]);

  return <div ref={ref} data-testid="solidity-view" className="h-full overflow-auto" />;
}
