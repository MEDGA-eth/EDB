import { HighlightStyle, syntaxHighlighting } from '@codemirror/language';
import { search } from '@codemirror/search';
import { Compartment, EditorState, RangeSetBuilder } from '@codemirror/state';
import {
  Decoration,
  EditorView,
  GutterMarker,
  gutter,
  lineNumbers,
} from '@codemirror/view';
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
  // Breakpoint gutter — fixed-width column, cursor flips to "pointer" so
  // clicks feel like the breakpoint affordance they actually are.
  '.cm-edb-bp-gutter': {
    width: '14px',
    cursor: 'pointer',
  },
  '.cm-edb-bp-gutter .cm-gutterElement': {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
  },
  '.cm-edb-bp-marker': {
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    lineHeight: '1',
  },
});

/**
 * Per-line breakpoint state used to render gutter markers. `enabled === false`
 * yields a dimmed grey dot; otherwise the marker is the danger-coloured dot
 * users associate with breakpoints in IDEs. Callers compute this list by
 * filtering the global breakpoint store to entries matching the editor's
 * `(addr, path)` pair.
 */
export interface BreakpointMarker {
  /** 1-indexed line number; matches `Source.line_number` in the wire format. */
  line: number;
  /** UI-only enable flag; defaults to true. */
  enabled: boolean;
}

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
  /**
   * Lines that should display a breakpoint dot in the dedicated bp-gutter.
   * Reconfigured live via a compartment when the array identity changes.
   * Empty / undefined hides the gutter entirely.
   */
  breakpoints?: BreakpointMarker[];
  /**
   * Click handler for the breakpoint gutter. Receives the 1-indexed line that
   * was clicked. The hook does NOT mutate any store directly — the host wires
   * this into `addBreakpoint` (or a toggle) so stores stay decoupled.
   */
  onToggleBreakpoint?(line: number): void;
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
 * GutterMarker rendering an inline breakpoint dot. Two variants:
 * - `enabled` → solid red circle keyed off `--color-danger`.
 * - `disabled` → muted grey circle keyed off `--color-fg-tertiary`.
 *
 * We render with a small SVG instead of a CSS pseudo-element so the marker
 * lines up with the gutter's text-baseline alignment regardless of font.
 */
class BreakpointGutterMarker extends GutterMarker {
  constructor(public readonly enabled: boolean) {
    super();
  }
  override eq(other: GutterMarker): boolean {
    return other instanceof BreakpointGutterMarker && other.enabled === this.enabled;
  }
  override toDOM(): HTMLElement {
    const span = document.createElement('span');
    span.className = 'cm-edb-bp-marker';
    span.setAttribute('data-enabled', this.enabled ? 'true' : 'false');
    span.innerHTML =
      '<svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">' +
      `<circle cx="5" cy="5" r="4" fill="${
        this.enabled ? 'var(--color-danger)' : 'var(--color-fg-tertiary)'
      }" /></svg>`;
    return span;
  }
}

const ENABLED_MARKER = new BreakpointGutterMarker(true);
const DISABLED_MARKER = new BreakpointGutterMarker(false);

/** Build a breakpoint-gutter extension keyed off the supplied markers. */
function makeBreakpointGutter(
  markers: BreakpointMarker[],
  onToggle?: (line: number) => void,
) {
  return gutter({
    class: 'cm-edb-bp-gutter',
    markers: (view) => {
      const builder = new RangeSetBuilder<GutterMarker>();
      // Sort + dedupe by line so RangeSetBuilder receives strictly-increasing
      // positions (it throws otherwise).
      const seen = new Map<number, BreakpointMarker>();
      for (const m of markers) {
        if (!seen.has(m.line)) seen.set(m.line, m);
      }
      const sorted = [...seen.values()].sort((a, b) => a.line - b.line);
      for (const m of sorted) {
        if (m.line < 1 || m.line > view.state.doc.lines) continue;
        const l = view.state.doc.line(m.line);
        builder.add(l.from, l.from, m.enabled ? ENABLED_MARKER : DISABLED_MARKER);
      }
      return builder.finish();
    },
    initialSpacer: () => ENABLED_MARKER,
    domEventHandlers: onToggle
      ? {
          mousedown: (view, line) => {
            const ln = view.state.doc.lineAt(line.from).number;
            onToggle(ln);
            return true;
          },
        }
      : undefined,
  });
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
  const { content, showLineNumbers, wordWrap, highlightLine, breakpoints, onToggleBreakpoint } = opts;
  const containerRef = useRef<HTMLDivElement | null>(null);
  const viewRef = useRef<EditorView | null>(null);
  const wrapCmpRef = useRef<Compartment>(new Compartment());
  const lnCmpRef = useRef<Compartment>(new Compartment());
  const hlCmpRef = useRef<Compartment>(new Compartment());
  const bpCmpRef = useRef<Compartment>(new Compartment());
  // Stash the latest toggle handler in a ref so the gutter compartment
  // doesn't need to be reconfigured every time the React owner re-renders
  // — it always invokes the freshest closure.
  const toggleRef = useRef<typeof onToggleBreakpoint>(onToggleBreakpoint);
  toggleRef.current = onToggleBreakpoint;

  useEffect(() => {
    if (!containerRef.current) return;
    const wrapCmp = wrapCmpRef.current;
    const lnCmp = lnCmpRef.current;
    const hlCmp = hlCmpRef.current;
    const bpCmp = bpCmpRef.current;
    const state = EditorState.create({
      doc: content,
      extensions: [
        // breakpoint gutter mounts to the LEFT of the line-number gutter so
        // dots sit in the spot users expect from VS Code / IntelliJ. Empty
        // when there's no toggle handler (we still render markers in that
        // mode since the gutter renders on its own).
        bpCmp.of(
          makeBreakpointGutter(breakpoints ?? [], (line) => toggleRef.current?.(line)),
        ),
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
    // Stamp a data-testid on the breakpoint gutter so playwright (and the
    // smoke script) can assert it independently of CodeMirror's internal
    // class names. The gutter is rendered synchronously after `new EditorView`
    // so the query runs after mount in the same tick.
    const bpGutterEl = containerRef.current.querySelector('.cm-edb-bp-gutter');
    if (bpGutterEl) bpGutterEl.setAttribute('data-testid', 'bp-gutter');
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

  // Reconfigure the breakpoint gutter when the marker list changes. We
  // serialise the markers into a stable string so consumers passing fresh
  // arrays each render don't trigger pointless reconfiguration.
  const bpKey = (breakpoints ?? [])
    .map((m) => `${m.line}:${m.enabled ? '1' : '0'}`)
    .sort()
    .join(',');
  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    view.dispatch({
      effects: bpCmpRef.current.reconfigure(
        makeBreakpointGutter(breakpoints ?? [], (line) => toggleRef.current?.(line)),
      ),
    });
    // bpKey captures the only meaningful change in `breakpoints`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [bpKey]);

  function revealOffset(byteOffset: number) {
    const view = viewRef.current;
    if (!view) return;
    const clamped = Math.max(0, Math.min(byteOffset, view.state.doc.length));
    const line = view.state.doc.lineAt(clamped);
    view.dispatch({ effects: EditorView.scrollIntoView(line.from, { y: 'center' }) });
  }

  return { containerRef, viewRef, revealOffset };
}
