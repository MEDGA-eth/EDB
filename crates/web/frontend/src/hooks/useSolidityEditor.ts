import { HighlightStyle, syntaxHighlighting } from '@codemirror/language';
import { search } from '@codemirror/search';
import { Compartment, EditorState } from '@codemirror/state';
import { EditorView, lineNumbers } from '@codemirror/view';
import { tags as t } from '@lezer/highlight';
import { solidity } from '@replit/codemirror-lang-solidity';
import { useEffect, useRef } from 'react';

/**
 * The shared theme + highlight configuration used by every read-only Solidity
 * code-mirror view in the app. Exported for tests that want to render against
 * the same configuration without re-initialising the editor.
 */
export const edbHighlight = HighlightStyle.define([
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

export const edbTheme = EditorView.theme({
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

export interface SolidityEditorOptions {
  /** Document content. Re-initialises the view when it changes. */
  content: string;
  /** Render line-number gutter? Reconfigured live. */
  showLineNumbers: boolean;
  /** Wrap long lines? Reconfigured live. */
  wordWrap: boolean;
}

export interface SolidityEditorHandle {
  /** Ref to attach to the host `<div>`. */
  containerRef: React.MutableRefObject<HTMLDivElement | null>;
  /** Imperative handle to the underlying EditorView (e.g. for openSearchPanel). */
  viewRef: React.MutableRefObject<EditorView | null>;
}

/**
 * Boots a read-only Solidity CodeMirror editor inside the returned
 * `containerRef`. Both the file-tab editor and the mobile code panel share
 * this hook so that the highlight + compartment plumbing lives in one place.
 *
 * The view is re-initialised when `content` changes, but `wordWrap` and
 * `showLineNumbers` are reconfigured via compartments without recreating the
 * view.
 */
export function useSolidityEditor(opts: SolidityEditorOptions): SolidityEditorHandle {
  const { content, showLineNumbers, wordWrap } = opts;
  const containerRef = useRef<HTMLDivElement | null>(null);
  const viewRef = useRef<EditorView | null>(null);
  const wrapCmpRef = useRef<Compartment>(new Compartment());
  const lnCmpRef = useRef<Compartment>(new Compartment());

  useEffect(() => {
    if (!containerRef.current) return;
    const wrapCmp = wrapCmpRef.current;
    const lnCmp = lnCmpRef.current;
    const state = EditorState.create({
      doc: content,
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
    const view = new EditorView({ state, parent: containerRef.current });
    viewRef.current = view;
    return () => {
      view.destroy();
      viewRef.current = null;
    };
    // re-initialise only when content truly changes; toggles are handled via
    // compartments below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [content]);

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

  return { containerRef, viewRef };
}
