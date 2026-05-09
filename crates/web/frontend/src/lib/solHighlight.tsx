import { Fragment, type ReactNode } from 'react';

/**
 * Lightweight, regex-based Solidity highlighter for the terminal echo.
 *
 * The full CodeMirror Solidity grammar is overkill for a single-line input
 * echo, so we walk the source once and emit a span for each token kind we
 * recognise. Anything we don't classify falls through verbatim. The colour
 * vars match the token palette used by the CodeMirror editor (so keywords
 * read identically in both surfaces).
 *
 * Order matters: comments → strings → numbers → keywords/types →
 * function-calls. Earlier patterns shadow later ones.
 */

interface Span {
  start: number;
  end: number;
  cssVar: string; // CSS variable from --color-syn-*
  weight?: 600 | 700;
  italic?: boolean;
}

const KEYWORDS = new Set<string>([
  'function', 'return', 'returns', 'if', 'else', 'for', 'while', 'do',
  'require', 'assert', 'revert', 'emit', 'new', 'delete', 'try', 'catch',
  'using', 'import', 'pragma', 'is', 'as', 'let', 'var', 'const', 'continue',
  'break', 'throw', 'unchecked', 'assembly', 'solidity',
]);

const DEF_KEYWORDS = new Set<string>([
  'contract', 'interface', 'library', 'struct', 'enum', 'event',
  'modifier', 'constructor', 'receive', 'fallback',
]);

const VISIBILITY = new Set<string>([
  'public', 'private', 'external', 'internal',
  'view', 'pure', 'payable', 'nonpayable',
  'override', 'virtual', 'abstract', 'immutable', 'constant',
]);

const STD_TYPES = new Set<string>([
  'uint', 'uint8', 'uint16', 'uint24', 'uint32', 'uint40', 'uint48', 'uint56',
  'uint64', 'uint72', 'uint80', 'uint88', 'uint96', 'uint104', 'uint112',
  'uint120', 'uint128', 'uint136', 'uint144', 'uint152', 'uint160', 'uint168',
  'uint176', 'uint184', 'uint192', 'uint200', 'uint208', 'uint216', 'uint224',
  'uint232', 'uint240', 'uint248', 'uint256',
  'int', 'int8', 'int16', 'int32', 'int64', 'int128', 'int256',
  'bool', 'address', 'string', 'bytes',
  'bytes1', 'bytes2', 'bytes4', 'bytes8', 'bytes16', 'bytes20', 'bytes32',
  'mapping', 'memory', 'storage', 'calldata',
]);

const ATOMS = new Set<string>(['true', 'false', 'null', 'undefined']);
const SELF = new Set<string>(['this', 'super', 'msg', 'tx', 'block', 'self']);

function classify(word: string): string | null {
  if (DEF_KEYWORDS.has(word)) return '--color-syn-keyword';
  if (KEYWORDS.has(word)) return '--color-syn-control';
  if (VISIBILITY.has(word)) return '--color-syn-modifier';
  if (STD_TYPES.has(word)) return '--color-syn-type-std';
  if (ATOMS.has(word)) return '--color-syn-atom';
  if (SELF.has(word)) return '--color-syn-self';
  if (/^[A-Z][A-Z0-9_]+$/.test(word)) return '--color-syn-constant';
  return null;
}

export function highlightSolidity(source: string, key?: string): ReactNode {
  if (!source) return null;
  const spans: Span[] = [];

  // 1. Comments — `//…` and `/*…*/` (multi-line not realistic for one-liners
  //    but cheap to support).
  for (const m of source.matchAll(/\/\/[^\n]*/g)) {
    if (typeof m.index === 'number')
      spans.push({ start: m.index, end: m.index + m[0].length, cssVar: '--color-syn-comment', italic: true });
  }
  for (const m of source.matchAll(/\/\*[\s\S]*?\*\//g)) {
    if (typeof m.index === 'number')
      spans.push({ start: m.index, end: m.index + m[0].length, cssVar: '--color-syn-comment', italic: true });
  }
  // 2. Strings — single, double, hex literal `hex"…"`.
  for (const m of source.matchAll(/(?:hex)?(["'])(?:\\.|(?!\1).)*\1/g)) {
    if (typeof m.index === 'number' && !insideAny(spans, m.index))
      spans.push({ start: m.index, end: m.index + m[0].length, cssVar: '--color-syn-string' });
  }
  // 3. Hex addresses (40 nibbles) get the type colour for distinct visual.
  for (const m of source.matchAll(/0x[0-9a-fA-F]{40}\b/g)) {
    if (typeof m.index === 'number' && !insideAny(spans, m.index))
      spans.push({ start: m.index, end: m.index + m[0].length, cssVar: '--color-syn-type' });
  }
  // 4. Other numeric literals (incl. shorter hex).
  for (const m of source.matchAll(/\b(0x[0-9a-fA-F]+|\d+(?:_\d+)*(?:\.\d+)?)\b/g)) {
    if (typeof m.index === 'number' && !insideAny(spans, m.index))
      spans.push({ start: m.index, end: m.index + m[0].length, cssVar: '--color-syn-number' });
  }
  // 5. Words → classify by lookup.
  for (const m of source.matchAll(/[a-zA-Z_$][\w$]*/g)) {
    if (typeof m.index !== 'number') continue;
    if (insideAny(spans, m.index)) continue;
    const word = m[0];
    const cssVar = classify(word);
    const after = source[m.index + word.length];
    // Function-call form: `name(` → function colour.
    if (!cssVar && after === '(') {
      spans.push({ start: m.index, end: m.index + word.length, cssVar: '--color-syn-func' });
      continue;
    }
    if (cssVar) {
      const weight: 600 | 700 | undefined = DEF_KEYWORDS.has(word) ? 700 : (cssVar === '--color-syn-control' ? 600 : undefined);
      spans.push({ start: m.index, end: m.index + word.length, cssVar, weight });
    }
  }

  spans.sort((a, b) => a.start - b.start || a.end - b.end);

  const out: ReactNode[] = [];
  let cursor = 0;
  for (let i = 0; i < spans.length; i += 1) {
    const s = spans[i];
    if (s.start < cursor) continue; // skip overlaps
    if (s.start > cursor) out.push(<Fragment key={`p-${i}`}>{source.slice(cursor, s.start)}</Fragment>);
    out.push(
      <span
        key={`s-${i}`}
        style={{
          color: `var(${s.cssVar})`,
          fontWeight: s.weight,
          fontStyle: s.italic ? 'italic' : undefined,
        }}
      >
        {source.slice(s.start, s.end)}
      </span>,
    );
    cursor = s.end;
  }
  if (cursor < source.length) out.push(<Fragment key="tail">{source.slice(cursor)}</Fragment>);
  return key ? <Fragment key={key}>{out}</Fragment> : <>{out}</>;
}

function insideAny(spans: Span[], idx: number): boolean {
  for (const s of spans) {
    if (idx >= s.start && idx < s.end) return true;
  }
  return false;
}
