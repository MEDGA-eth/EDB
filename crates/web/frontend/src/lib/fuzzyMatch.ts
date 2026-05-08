/**
 * Tiny fuzzy matcher. Returns a numeric score (higher = better) when every
 * char in `needle` appears in order inside `haystack`, or null if not.
 *
 * Scoring rewards: prefix matches, contiguous runs, word-boundary hits.
 * It's not perfect but it's enough to drive a command palette without
 * pulling in another dependency.
 */
export function fuzzyScore(haystack: string, needle: string): number | null {
  if (needle.length === 0) return 0;
  const h = haystack.toLowerCase();
  const n = needle.toLowerCase();

  // Cheap exact-substring shortcut.
  const idx = h.indexOf(n);
  if (idx !== -1) {
    // prefix gets a hefty boost
    const prefixBoost = idx === 0 ? 200 : 0;
    const wordStartBoost = idx > 0 && /[\s/_.\-:]/.test(h[idx - 1] ?? '') ? 60 : 0;
    return 1000 - idx + prefixBoost + wordStartBoost + n.length * 8;
  }

  // Subsequence walk.
  let hi = 0;
  let ni = 0;
  let score = 0;
  let lastMatch = -2;
  while (hi < h.length && ni < n.length) {
    if (h[hi] === n[ni]) {
      const isWordStart = hi === 0 || /[\s/_.\-:]/.test(h[hi - 1] ?? '');
      const contiguous = hi === lastMatch + 1;
      score += 4;
      if (isWordStart) score += 12;
      if (contiguous) score += 8;
      lastMatch = hi;
      ni += 1;
    }
    hi += 1;
  }
  if (ni < n.length) return null;
  // penalize long haystacks slightly
  score -= h.length * 0.05;
  return score;
}

export interface RankItem<T> {
  item: T;
  score: number;
}

export function rankBy<T>(
  items: T[],
  query: string,
  keyFn: (t: T) => string,
): RankItem<T>[] {
  if (!query) return items.map((item) => ({ item, score: 0 }));
  const out: RankItem<T>[] = [];
  for (const item of items) {
    const s = fuzzyScore(keyFn(item), query);
    if (s !== null) out.push({ item, score: s });
  }
  out.sort((a, b) => b.score - a.score);
  return out;
}
