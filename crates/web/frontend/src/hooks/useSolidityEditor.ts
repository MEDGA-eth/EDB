import { HighlightStyle, syntaxHighlighting } from '@codemirror/language';
import { search } from '@codemirror/search';
import { Compartment, EditorState, RangeSetBuilder } from '@codemirror/state';
import { Decoration, EditorView, lineNumbers } from '@codemirror/view';
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
  // Control-flow vs definition keywords get distinctive colours so the eye
  // can tell `if/else/return` from `function/contract/struct`.
  { tag: t.controlKeyword, color: 'var(--color-syn-control)', fontWeight: '600' },
  { tag: t.definitionKeyword, color: 'var(--color-syn-keyword)', fontWeight: '700' },
  { tag: t.modifier, color: 'var(--color-syn-modifier)' },
  { tag: t.self, color: 'var(--color-syn-self)' },
  { tag: [t.string, t.special(t.string)], color: 'var(--color-syn-string)' },
  { tag: t.escape, color: 'var(--color-syn-escape)' },
  { tag: [t.number, t.bool, t.null], color: 'var(--color-syn-number)' },
  { tag: [t.bool, t.null, t.atom], color: 'var(--color-syn-atom)' },
  { tag: t.constant(t.variableName), color: 'var(--color-syn-constant)' },
  { tag: t.comment, color: 'var(--color-syn-comment)', fontStyle: 'italic' },
  { tag: t.docComment, color: 'var(--color-syn-doc)', fontStyle: 'italic' },
  { tag: [t.typeName, t.className, t.namespace], color: 'var(--color-syn-type)' },
  { tag: t.standard(t.typeName), color: 'var(--color-syn-type-std)' },
  {
    tag: [t.function(t.variableName), t.function(t.propertyName)],
    color: 'var(--color-syn-func)',
  },
  { tag: [t.operator, t.operatorKeyword], color: 'var(--color-fg)' },
  { tag: t.variableName, color: 'var(--color-fg)' },
  { tag: t.propertyName, color: 'var(--color-fg-secondary)' },
  { tag: t.bracket, color: 'var(--color-fg-tertiary)' },
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
  /**
   * 1-indexed line number to highlight (subtle accent background) and scroll
   * into view. Used by the editor area to track the current snapshot's
   * source location. Pass `undefined` to clear.
   */
  highlightLine?: number;
}

export interface SolidityEditorHandle {
  /** Ref to attach to the host `<div>`. */
  containerRef: React.MutableRefObject<HTMLDivElement | null>;
  /** Imperative handle to the underlying EditorView (e.g. for openSearchPanel). */
  viewRef: React.MutableRefObject<EditorView | null>;
  /** Imperative scroll-to-byte-offset (1-indexed line number resolved internally). */
  revealOffset(byteOffset: number): void;
}

const lineHighlight = Decoration.line({
  attributes: { class: 'cm-edb-current-line', 'data-edb-current': 'true' },
});

function buildHighlight(view: EditorView, line: number | undefined) {
  const builder = new RangeSetBuilder<Decoration>();
  if (line && line > 0 && line <= view.state.doc.lines) {
    const l = view.state.doc.line(line);
    builder.add(l.from, l.from, lineHighlight);
  }
  return builder.finish();
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
  const { content, showLineNumbers, wordWrap, highlightLine } = opts;
  const containerRef = useRef<HTMLDivElement | null>(null);
  const viewRef = useRef<EditorView | null>(null);
  const wrapCmpRef = useRef<Compartment>(new Compartment());
  const lnCmpRef = useRef<Compartment>(new Compartment());
  const hlCmpRef = useRef<Compartment>(new Compartment());

  useEffect(() => {
    if (!containerRef.current) return;
    const wrapCmp = wrapCmpRef.current;
    const lnCmp = lnCmpRef.current;
    const hlCmp = hlCmpRef.current;
    const state = EditorState.create({
      doc: content,
      extensions: [
        lnCmp.of(showLineNumbers ? [lineNumbers()] : []),
        solidity,
        syntaxHighlighting(edbHighlight),
        edbTheme,
        search(),
        wrapCmp.of(wordWrap ? [EditorView.lineWrapping] : []),
        // current-line highlight, reconfigured imperatively when the active
        // snapshot resolves to a new (file, line).
        hlCmp.of([]),
        EditorView.editable.of(false),
      ],
    });
    const view = new EditorView({ state, parent: containerRef.current });
    viewRef.current = view;
    if (typeof highlightLine === 'number') {
      view.dispatch({
        effects: hlCmpRef.current.reconfigure(
          EditorView.decorations.of(buildHighlight(view, highlightLine)),
        ),
      });
    }
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

  // Reconfigure highlight + scroll into view when the active line changes.
  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    view.dispatch({
      effects: hlCmpRef.current.reconfigure(
        highlightLine
          ? EditorView.decorations.of(buildHighlight(view, highlightLine))
          : [],
      ),
    });
    if (highlightLine && highlightLine > 0 && highlightLine <= view.state.doc.lines) {
      const l = view.state.doc.line(highlightLine);
      view.dispatch({ effects: EditorView.scrollIntoView(l.from, { y: 'center' }) });
    }
  }, [highlightLine]);

  function revealOffset(byteOffset: number) {
    const view = viewRef.current;
    if (!view) return;
    const clamped = Math.max(0, Math.min(byteOffset, view.state.doc.length));
    const line = view.state.doc.lineAt(clamped);
    view.dispatch({ effects: EditorView.scrollIntoView(line.from, { y: 'center' }) });
  }

  return { containerRef, viewRef, revealOffset };
}
