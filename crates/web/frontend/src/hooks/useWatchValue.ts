import { useQuery } from '@tanstack/react-query';
import { rpc } from '../lib/rpc';
import { EvalResult, type EvalResult as EvalResultType } from '../lib/types';

/**
 * Watch a single Solidity expression at a particular snapshot. Mirrors the
 * cache key shape used by {@link useEvalExpr} (`['eval', snapshotId, expr]`)
 * so a terminal evaluation primes the watch panel without a refetch (and
 * vice versa). The query auto-fires on snapshot change because the key
 * embeds `snapshotId`.
 */
export function useWatchValue(expr: string, snapshotId: number) {
  return useQuery<EvalResultType>({
    queryKey: ['eval', snapshotId, expr] as const,
    queryFn: () => rpc('edb_evalOnSnapshot', EvalResult, [snapshotId, expr]),
    enabled: expr.length > 0 && snapshotId >= 0,
    // Each snapshot's value is immutable, no need to refetch on focus.
    staleTime: Infinity,
  });
}
