import { describe, expect, test } from 'bun:test';
import { renderToString } from 'react-dom/server';
import { highlightSolidity } from './solHighlight';

function html(source: string): string {
  // Render the React node tree to a static HTML string and assert on the
  // resulting markup. Cheaper than a DOM mount for what's effectively a
  // pure-function check.
  return renderToString(<>{highlightSolidity(source)}</>);
}

describe('highlightSolidity', () => {
  test('returns null for empty input', () => {
    expect(highlightSolidity('')).toBeNull();
  });

  test('passes plain text through verbatim', () => {
    expect(html('hello world')).toBe('hello world');
  });

  test('keywords get the control colour', () => {
    const out = html('return msg.sender');
    expect(out).toContain('--color-syn-control');
    expect(out).toContain('return');
    // self/this words use --color-syn-self
    expect(out).toContain('--color-syn-self');
  });

  test('definition keywords get a 700-weight colour', () => {
    const out = html('contract Foo {}');
    expect(out).toContain('--color-syn-keyword');
    expect(out).toContain('font-weight:700');
  });

  test('standard types get the type-std colour', () => {
    const out = html('uint256 x = 1');
    expect(out).toContain('--color-syn-type-std');
    expect(out).toContain('--color-syn-number');
  });

  test('addresses get the type colour, not the number colour', () => {
    const out = html('to == 0x1111111111111111111111111111111111111111');
    expect(out).toContain('--color-syn-type');
    // Address pattern claims the whole hex literal — no overlap with the
    // number rule.
    const numHits = (out.match(/--color-syn-number/g) || []).length;
    expect(numHits).toBe(0);
  });

  test('strings claim their full quoted span', () => {
    const out = html('require(true, "boom")');
    expect(out).toContain('--color-syn-string');
    // ReactDOMServer escapes the quotes — match the entity-form.
    expect(out).toContain('&quot;boom&quot;');
  });

  test('function calls — name immediately before `(` — get the func colour', () => {
    const out = html('balanceOf(account)');
    expect(out).toContain('--color-syn-func');
    expect(out).toContain('balanceOf');
  });

  test('UPPER_SNAKE words are styled as constants', () => {
    const out = html('return MAX_SUPPLY');
    expect(out).toContain('--color-syn-constant');
    expect(out).toContain('MAX_SUPPLY');
  });

  test('comments are coloured italic', () => {
    const out = html('1 + 1 // a sum');
    expect(out).toContain('--color-syn-comment');
    expect(out).toContain('font-style:italic');
  });
});
