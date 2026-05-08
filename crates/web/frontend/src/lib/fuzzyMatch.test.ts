import { describe, expect, test } from 'bun:test';
import { fuzzyScore, rankBy } from './fuzzyMatch';

describe('fuzzyScore', () => {
  test('empty needle scores 0', () => {
    expect(fuzzyScore('foo', '')).toBe(0);
  });

  test('non-matching subsequence returns null', () => {
    expect(fuzzyScore('abc', 'xyz')).toBeNull();
  });

  test('exact prefix beats other matches', () => {
    const a = fuzzyScore('foobar', 'foo')!;
    const b = fuzzyScore('barfoo', 'foo')!;
    expect(a).toBeGreaterThan(b);
  });

  test('contiguous + word-start gets bonus', () => {
    const a = fuzzyScore('user/main.sol', 'main')!;
    const b = fuzzyScore('mathmainmix', 'main')!;
    expect(a).toBeGreaterThan(b);
  });

  test('subsequence with gaps still scores when no contiguous run exists', () => {
    expect(fuzzyScore('abXcYd', 'abcd')).not.toBeNull();
  });
});

describe('rankBy', () => {
  test('returns matches in descending score', () => {
    const items = ['barfoo', 'foobar', 'fooz'];
    const out = rankBy(items, 'foo', (s) => s);
    expect(out.map((r) => r.item)).toEqual(['foobar', 'fooz', 'barfoo']);
  });

  test('omits non-matches when query non-empty', () => {
    const out = rankBy(['cat', 'foo'], 'zzz', (s) => s);
    expect(out).toEqual([]);
  });

  test('returns all items when query is empty', () => {
    const out = rankBy(['a', 'b'], '', (s) => s);
    expect(out.length).toBe(2);
  });
});
